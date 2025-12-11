// ESNODE | Source Available BUSL-1.1 | Copyright (c) 2024 Estimatedstocks AB
use async_trait::async_trait;
#[cfg(feature = "gpu")]
use nvml_wrapper::{
    bitmasks::device::ThrottleReasons,
    bitmasks::nv_link::PacketTypes,
    enum_wrappers::device::{Clock, EccCounter, MemoryError, PcieUtilCounter, TemperatureSensor},
    enum_wrappers::nv_link::{ErrorCounter as NvLinkErrorCounter, UtilizationCountUnit},
    enums::device::PcieLinkMaxSpeed,
    enums::nv_link::Counter as NvLinkCounter,
    struct_wrappers::nv_link::UtilizationControl,
    Nvml,
};
#[cfg(all(feature = "gpu", target_os = "linux"))]
use nvml_wrapper::bitmasks::event::EventTypes;
use prometheus::GaugeVec;
use std::collections::HashMap;
use std::collections::HashSet;
#[cfg(feature = "gpu")]
use std::time::Instant;
#[cfg(all(feature = "gpu", target_os = "linux"))]
use tokio::sync::mpsc;

#[cfg(all(feature = "gpu", feature = "gpu-nvml-ffi"))]
use anyhow::Result;

use crate::collectors::Collector;
#[cfg(all(feature = "gpu", target_os = "linux"))]
use crate::event_worker::spawn_event_worker;
use crate::metrics::MetricsRegistry;
use crate::state::{
    FabricLink, FabricLinkType, GpuCapabilities, GpuHealth, GpuIdentity, GpuStatus, GpuTopo,
    GpuVendor, StatusState,
};
#[cfg(all(feature = "gpu", feature = "gpu-nvml-ffi"))]
use crate::state::{ComputeInstanceNode, GpuInstanceNode, MigTree};
#[cfg(all(feature = "gpu", target_os = "linux"))]
use nvml_wrapper::error::NvmlError;
use crate::config::AgentConfig;
#[cfg(all(feature = "gpu", feature = "gpu-nvml-ffi"))]
use nvml_wrapper_sys::bindings::{
    nvmlDeviceGetDeviceHandleFromMigDeviceHandle, nvmlDeviceGetMaxMigDeviceCount,
    nvmlDeviceGetMigDeviceHandleByIndex, nvmlDeviceGetMigMode,
};

pub struct GpuCollector {
    #[cfg(feature = "gpu")]
    nvml: Option<Nvml>,
    #[cfg(feature = "gpu")]
    ecc_prev: HashMap<String, u64>,
    #[cfg(feature = "gpu")]
    last_power: HashMap<u32, (f64, Instant)>,
    #[cfg(feature = "gpu")]
    last_pcie_sample: HashMap<u32, Instant>,
    #[cfg(feature = "gpu")]
    last_pcie_replay: HashMap<u32, u32>,
    #[cfg(feature = "gpu")]
    nvlink_util_prev: HashMap<(u32, u32), (u64, u64)>,
    #[cfg(feature = "gpu")]
    nvlink_err_prev: HashMap<(u32, u32, String), u64>,
    #[cfg(feature = "gpu")]
    enable_mig: bool,
    #[cfg(feature = "gpu")]
    enable_events: bool,
    #[cfg(feature = "gpu")]
    #[allow(dead_code)]
    enable_amd: bool,
    #[cfg(feature = "gpu")]
    visible_filter: Option<HashSet<String>>,
    #[cfg(feature = "gpu")]
    mig_config_filter: Option<HashSet<String>>,
    #[cfg(feature = "gpu")]
    k8s_mode: bool,
    #[cfg(feature = "gpu")]
    resource_prefix: &'static str,
    #[cfg(all(feature = "gpu", target_os = "linux"))]
    event_rx: Option<mpsc::Receiver<crate::event_worker::EventRecord>>,
    status: StatusState,
}

impl GpuCollector {
    pub fn new(status: StatusState, config: &AgentConfig) -> (Self, Option<String>) {
        #[cfg(feature = "gpu")]
        {
            let env_visible = std::env::var("NVIDIA_VISIBLE_DEVICES").ok();
            let env_mig_config = std::env::var("NVIDIA_MIG_CONFIG_DEVICES").ok();
            let visible_filter = build_filter(
                config
                    .gpu_visible_devices
                    .as_deref()
                    .or(env_visible.as_deref()),
            );
            let mig_cfg_filter = build_filter(
                config
                    .mig_config_devices
                    .as_deref()
                    .or(env_mig_config.as_deref()),
            );
            #[cfg(all(feature = "gpu", target_os = "linux"))]
            let (event_tx, event_rx) = if config.enable_gpu_events {
                let (tx, rx) = mpsc::channel::<crate::event_worker::EventRecord>(256);
                (Some(tx), Some(rx))
            } else {
                (None, None)
            };
            #[cfg(not(all(feature = "gpu", target_os = "linux")))]
            let (_event_tx, _event_rx): (Option<()>, Option<()>) = (None, None);
            match Nvml::init() {
                Ok(nvml) => {
                    #[cfg(all(feature = "gpu", target_os = "linux"))]
                    if let Some(tx) = event_tx.clone() {
                        spawn_event_worker(tx, visible_filter.clone());
                    }
                    (
                        Self {
                            nvml: Some(nvml),
                            ecc_prev: HashMap::new(),
                            last_power: HashMap::new(),
                        last_pcie_sample: HashMap::new(),
                        last_pcie_replay: HashMap::new(),
                        nvlink_util_prev: HashMap::new(),
                        nvlink_err_prev: HashMap::new(),
                        enable_mig: config.enable_gpu_mig,
                        enable_events: config.enable_gpu_events,
                        visible_filter: visible_filter.clone(),
                        mig_config_filter: mig_cfg_filter.clone(),
                            k8s_mode: config.k8s_mode,
                            resource_prefix: if config.k8s_mode {
                                "nvidia.com"
                            } else {
                                "esnode.co"
                            },
                            enable_amd: config.enable_gpu_amd,
                            #[cfg(all(feature = "gpu", target_os = "linux"))]
                            event_rx,
                            status,
                        },
                        None,
                    )
                }
                Err(e) => {
                    (
                        Self {
                            nvml: None,
                            ecc_prev: HashMap::new(),
                            last_power: HashMap::new(),
                            last_pcie_sample: HashMap::new(),
                            last_pcie_replay: HashMap::new(),
                            nvlink_util_prev: HashMap::new(),
                            nvlink_err_prev: HashMap::new(),
                            enable_mig: config.enable_gpu_mig,
                            enable_events: config.enable_gpu_events,
                            visible_filter: build_filter(
                                config
                                    .gpu_visible_devices
                                    .as_deref()
                                    .or(env_visible.as_deref()),
                            ),
                            mig_config_filter: build_filter(
                                config
                                    .mig_config_devices
                                    .as_deref()
                                    .or(env_mig_config.as_deref()),
                            ),
                            k8s_mode: config.k8s_mode,
                            resource_prefix: if config.k8s_mode {
                                "nvidia.com"
                            } else {
                                "esnode.co"
                            },
                            enable_amd: config.enable_gpu_amd,
                            #[cfg(all(feature = "gpu", target_os = "linux"))]
                            event_rx: None,
                            status,
                        },
                        Some(format!("GPU collector disabled: {}", e)),
                    )
                }
            }
        }

        #[cfg(not(feature = "gpu"))]
        {
            (
                Self { status },
                Some("GPU support not compiled in".to_string()),
            )
        }
    }
}

#[async_trait]
impl Collector for GpuCollector {
    fn name(&self) -> &'static str {
        "gpu"
    }

    async fn collect(&mut self, metrics: &MetricsRegistry) -> anyhow::Result<()> {
        #[cfg(feature = "gpu")]
        {
            let Some(nvml) = &self.nvml else {
                return Ok(());
            };

            let count = nvml.device_count()?;
            let mut statuses: Vec<GpuStatus> = Vec::new();
            let mut uuid_to_index: HashMap<String, String> = HashMap::new();
            // Drain any pending events from the async task.
            #[cfg(all(feature = "gpu", target_os = "linux"))]
            {
                if let Some(rx) = &mut self.event_rx {
                    while let Ok(ev) = rx.try_recv() {
                        let labels = &[ev.uuid.as_str(), ev.index.as_str(), ev.kind.as_str()];
                        metrics.gpu_events_total.with_label_values(labels).inc();
                        metrics
                            .gpu_last_event_unix_ms
                            .with_label_values(labels)
                            .set(ev.ts_ms as f64);
                        match ev.kind.as_str() {
                            "xid" => {
                                metrics
                                    .gpu_xid_errors_total
                                    .with_label_values(labels)
                                    .inc();
                                metrics
                                    .gpu_last_xid_code
                                    .with_label_values(labels)
                                    .set(ev.xid_code.unwrap_or(-1) as f64);
                            }
                            "ecc_single" => {
                                metrics
                                    .gpu_ecc_corrected_total
                                    .with_label_values(labels)
                                    .inc();
                            }
                            "ecc_double" => {
                                metrics
                                    .gpu_ecc_uncorrected_total
                                    .with_label_values(labels)
                                    .inc();
                            }
                            _ => {}
                        }
                    }
                }
            }
            #[cfg(target_os = "linux")]
            let mut event_set = if self.enable_events {
                nvml.create_event_set().ok()
            } else {
                None
            };
            #[cfg(not(target_os = "linux"))]
            let event_set: Option<()> = None;
            #[cfg(not(target_os = "linux"))]
            let _ = &event_set;
            let events_enabled = self.enable_events;
            #[cfg(not(target_os = "linux"))]
            if events_enabled {
                tracing::debug!("GPU event polling requested but not supported on this platform; skipping");
            }
            for idx in 0..count {
                let device = nvml.device_by_index(idx)?;
                let gpu_label = idx.to_string();
                let uuid_string = device.uuid().unwrap_or_else(|_| format!("GPU-{}", gpu_label));
                if let Some(filter) = &self.visible_filter {
                    if !filter.contains(&uuid_string) && !filter.contains(&gpu_label) {
                        continue;
                    }
                }
                if self.enable_mig {
                    if let Some(filter) = &self.mig_config_filter {
                        if !filter.contains(&uuid_string) && !filter.contains(&gpu_label) {
                            continue;
                        }
                    }
                }
                let compat_label = if self.k8s_mode {
                    k8s_resource_name(self.resource_prefix, None)
                } else {
                    gpu_label.clone()
                };
                uuid_to_index.insert(uuid_string.clone(), gpu_label.clone());
                #[cfg(target_os = "linux")]
                {
                    if let Some(set) = event_set.take() {
                        let events = EventTypes::SINGLE_BIT_ECC_ERROR
                            | EventTypes::DOUBLE_BIT_ECC_ERROR
                            | EventTypes::CRITICAL_XID_ERROR
                            | EventTypes::PSTATE_CHANGE
                            | EventTypes::CLOCK_CHANGE;
                        let new_set = device.register_events(events, set).ok();
                        event_set = new_set;
                    }
                }
                let uuid_label = uuid_string.as_str();
                let now = Instant::now();
                metrics
                    .pcie_bandwidth_percent
                    .with_label_values(&[uuid_label, gpu_label.as_str()])
                    .set(0.0);

                // Identity/topology
                let identity = {
                    let pci = device.pci_info().ok();
                    let driver_version = nvml.sys_driver_version().ok();
                    let nvml_version = nvml.sys_nvml_version().ok();
                    let cuda_driver_version = nvml.sys_cuda_driver_version().ok();
                    let pci_id = pci.as_ref().map(|p| p.pci_device_id);
                    let pci_sub = pci.as_ref().map(|p| p.pci_sub_system_id);
                    Some(GpuIdentity {
                        pci_bus_id: pci.as_ref().map(|p| p.bus_id.clone()),
                        pci_domain: pci.as_ref().map(|p| p.domain),
                        pci_bus: pci.as_ref().map(|p| p.bus),
                        pci_device: pci.as_ref().map(|p| p.device),
                        pci_function: None,
                        pci_gen: None,
                        pci_link_width: None,
                        driver_version,
                        nvml_version,
                        cuda_driver_version,
                        device_id: pci_id,
                        subsystem_id: pci_sub.flatten(),
                        board_id: None,
                        numa_node: None,
                    })
                };
                let topo = {
                    let gen = device.current_pcie_link_gen().ok();
                    let width = device.current_pcie_link_width().ok();
                    Some(GpuTopo {
                        pci_link_gen: gen,
                        pci_link_width: width,
                    })
                };
                let mut health = GpuHealth::default();
                let mut status = GpuStatus {
                    uuid: Some(uuid_string.clone()),
                    gpu: gpu_label.clone(),
                    vendor: Some(GpuVendor::Nvidia),
                    capabilities: Some(GpuCapabilities {
                        mig: self.enable_mig,
                        sriov: false,
                        mcm_tiles: false,
                    }),
                    identity,
                    topo,
                    health: None,
                    ..Default::default()
                };

                if let Ok(util) = device.utilization_rates() {
                    metrics
                        .gpu_utilization_percent
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(util.gpu as f64);
                    if self.k8s_mode {
                        metrics
                            .gpu_utilization_percent_compat
                            .with_label_values(&[compat_label.as_str()])
                            .set(util.gpu as f64);
                    }
                    status.util_percent = Some(util.gpu as f64);
                }

                if let Ok(memory) = device.memory_info() {
                    metrics
                        .gpu_memory_total_bytes
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(memory.total as f64);
                    metrics
                        .gpu_memory_used_bytes
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(memory.used as f64);
                    if self.k8s_mode {
                        metrics
                            .gpu_memory_used_bytes_compat
                            .with_label_values(&[compat_label.as_str()])
                            .set(memory.used as f64);
                    }
                    status.memory_total_bytes = Some(memory.total as f64);
                    status.memory_used_bytes = Some(memory.used as f64);
                }

                if let Ok(temp) = device.temperature(TemperatureSensor::Gpu) {
                    metrics
                        .gpu_temperature_celsius
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(temp as f64);
                    if self.k8s_mode {
                        metrics
                            .gpu_temperature_celsius_compat
                            .with_label_values(&[compat_label.as_str()])
                            .set(temp as f64);
                    }
                    status.temperature_celsius = Some(temp as f64);
                }

                if let Ok(power) = device.power_usage() {
                    metrics
                        .gpu_power_watts
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(power as f64 / 1000.0);
                    if self.k8s_mode {
                        metrics
                            .gpu_power_watts_compat
                            .with_label_values(&[compat_label.as_str()])
                            .set(power as f64 / 1000.0);
                    }
                    let watts = power as f64 / 1000.0;
                    status.power_watts = Some(watts);
                    if let Some((prev_watts, ts)) = self.last_power.get(&idx) {
                        let dt = now.saturating_duration_since(*ts).as_secs_f64();
                        if dt > 0.0 {
                            let energy = (prev_watts * dt).floor() as u64;
                            metrics
                                .gpu_energy_joules_total
                                .with_label_values(&[uuid_label, gpu_label.as_str()])
                                .inc_by(energy);
                        }
                    }
                    self.last_power.insert(idx, (watts, now));
                }

                if let Ok(limit) = device.power_management_limit() {
                    metrics
                        .gpu_power_limit_watts
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(limit as f64 / 1000.0);
                }

                if let Ok(fan) = device.fan_speed(0) {
                    metrics
                        .gpu_fan_speed_percent
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(fan as f64);
                    status.fan_percent = Some(fan as f64);
                }

                if let Ok(sm_clock) = device.clock_info(Clock::SM) {
                    metrics
                        .gpu_clock_sm_mhz
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(sm_clock as f64);
                    status.clock_sm_mhz = Some(sm_clock as f64);
                }

                if let Ok(mem_clock) = device.clock_info(Clock::Memory) {
                    metrics
                        .gpu_clock_mem_mhz
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(mem_clock as f64);
                    status.clock_mem_mhz = Some(mem_clock as f64);
                }

                if let Ok(gfx_clock) = device.clock_info(Clock::Graphics) {
                    metrics
                        .gpu_clock_graphics_mhz
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(gfx_clock as f64);
                }
                if let Ok(pstate) = device.performance_state() {
                    let p_val = pstate as u32;
                    metrics
                        .gpu_pstate
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(p_val as f64);
                    health.pstate = Some(p_val);
                }
                if let Ok(bar1) = device.bar1_memory_info() {
                    metrics
                        .gpu_bar1_total_bytes
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(bar1.total as f64);
                    metrics
                        .gpu_bar1_used_bytes
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(bar1.used as f64);
                    health.bar1_total_bytes = Some(bar1.total);
                    health.bar1_used_bytes = Some(bar1.used);
                }
                if let Ok(enc_info) = device.encoder_utilization() {
                    metrics
                        .gpu_encoder_utilization_percent
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(enc_info.utilization as f64);
                    health.encoder_util_percent = Some(enc_info.utilization as f64);
                }
                if let Ok(dec_info) = device.decoder_utilization() {
                    metrics
                        .gpu_decoder_utilization_percent
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(dec_info.utilization as f64);
                    health.decoder_util_percent = Some(dec_info.utilization as f64);
                }

                // ECC and throttle reasons not available in nvml-wrapper 0.9; skip gracefully.
                for (counter, label) in [
                    (EccCounter::Volatile, "volatile"),
                    (EccCounter::Aggregate, "aggregate"),
                ] {
                    let corrected =
                        device.total_ecc_errors(MemoryError::Corrected, counter.clone());
                    let uncorrected =
                        device.total_ecc_errors(MemoryError::Uncorrected, counter.clone());
                    if let (Ok(c), Ok(u)) = (corrected, uncorrected) {
                        let total = c.saturating_add(u);
                        let key = format!("{}:{}", gpu_label, label);
                        let prev = *self.ecc_prev.get(&key).unwrap_or(&0);
                        if total >= prev {
                            metrics
                                .gpu_ecc_errors_total
                                .with_label_values(&[uuid_label, gpu_label.as_str(), label])
                                .inc_by(total - prev);
                        }
                        self.ecc_prev.insert(key, total);
                    } else {
                        // keep series visible even if call is unsupported
                        metrics
                            .gpu_ecc_errors_total
                            .with_label_values(&[uuid_label, gpu_label.as_str(), label])
                            .inc_by(0);
                    }
                }
                if let Ok(ecc_state) = device.is_ecc_enabled() {
                    metrics
                        .gpu_ecc_mode
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(if ecc_state.currently_enabled { 1.0 } else { 0.0 });
                    health.ecc_mode = Some(if ecc_state.currently_enabled {
                        "enabled".to_string()
                    } else {
                        "disabled".to_string()
                    });
                }
                if let Ok(reasons) = device.current_throttle_reasons() {
                    let thermal = reasons.intersects(
                        ThrottleReasons::HW_THERMAL_SLOWDOWN | ThrottleReasons::SW_THERMAL_SLOWDOWN,
                    );
                    let power = reasons.intersects(
                        ThrottleReasons::HW_POWER_BRAKE_SLOWDOWN | ThrottleReasons::SW_POWER_CAP,
                    );
                    set_throttle_metric(
                        &metrics.gpu_throttle_reason,
                        uuid_label,
                        gpu_label.as_str(),
                        "thermal",
                        thermal,
                    );
                    set_throttle_metric(
                        &metrics.gpu_throttle_reason,
                        uuid_label,
                        gpu_label.as_str(),
                        "power",
                        power,
                    );
                    set_throttle_metric(
                        &metrics.gpu_throttle_reason,
                        uuid_label,
                        gpu_label.as_str(),
                        "other",
                        !(thermal || power),
                    );
                    let mut reason_list = Vec::new();
                    if thermal {
                        reason_list.push("thermal".to_string());
                    }
                    if power {
                        reason_list.push("power".to_string());
                    }
                    health.throttle_reasons = reason_list;
                } else {
                    set_throttle_metric(
                        &metrics.gpu_throttle_reason,
                        uuid_label,
                        gpu_label.as_str(),
                        "thermal",
                        false,
                    );
                    set_throttle_metric(
                        &metrics.gpu_throttle_reason,
                        uuid_label,
                        gpu_label.as_str(),
                        "power",
                        false,
                    );
                    set_throttle_metric(
                        &metrics.gpu_throttle_reason,
                        uuid_label,
                        gpu_label.as_str(),
                        "other",
                        false,
                    );
                }

                // Initialize always-on counters for compatibility.
                metrics
                    .gpu_energy_joules_total
                    .with_label_values(&[uuid_label, gpu_label.as_str()])
                    .inc_by(0);
                #[cfg(all(feature = "gpu-nvml-ffi-ext", feature = "gpu"))]
                {
                    if let Ok(field_vals) = crate::nvml_ext::get_field_values(
                        unsafe { device.handle() },
                        &[
                            crate::nvml_ext::field::FI_DEV_PCIE_COUNT_CORRECTABLE_ERRORS,
                            crate::nvml_ext::field::FI_DEV_PCIE_COUNT_NON_FATAL_ERROR,
                            crate::nvml_ext::field::FI_DEV_PCIE_COUNT_FATAL_ERROR,
                        ],
                    ) {
                        if let Some(corr) =
                            field_vals.get(crate::nvml_ext::field::FI_DEV_PCIE_COUNT_CORRECTABLE_ERRORS)
                        {
                            metrics
                                .gpu_pcie_correctable_errors_total
                                .with_label_values(&[uuid_label, gpu_label.as_str()])
                                .inc_by(corr.max(0) as u64);
                        }
                        let non_fatal = field_vals
                            .get(crate::nvml_ext::field::FI_DEV_PCIE_COUNT_NON_FATAL_ERROR)
                            .unwrap_or(0);
                        let fatal = field_vals
                            .get(crate::nvml_ext::field::FI_DEV_PCIE_COUNT_FATAL_ERROR)
                            .unwrap_or(0);
                        let uncorrectable = (fatal + non_fatal).max(0) as u64;
                        metrics
                            .gpu_pcie_uncorrectable_errors_total
                            .with_label_values(&[uuid_label, gpu_label.as_str()])
                            .inc_by(uncorrectable);
                    }
                    if let Ok(ext) = crate::nvml_ext::pcie_ext_counters(unsafe { device.handle() }) {
                        if let Some(c) = ext.correctable_errors {
                            metrics
                                .gpu_pcie_correctable_errors_total
                                .with_label_values(&[uuid_label, gpu_label.as_str()])
                                .inc_by(c);
                        }
                        if let Some(a) = ext.atomic_requests {
                            metrics
                                .gpu_pcie_atomic_requests_total
                                .with_label_values(&[uuid_label, gpu_label.as_str()])
                                .inc_by(a);
                        }
                    }
                }
                #[cfg(not(all(feature = "gpu-nvml-ffi-ext", feature = "gpu")))]
                {
                    metrics
                        .gpu_pcie_correctable_errors_total
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .inc_by(0);
                    metrics
                        .gpu_pcie_atomic_requests_total
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .inc_by(0);
                }
                metrics
                    .gpu_copy_utilization_percent
                    .with_label_values(&[uuid_label, gpu_label.as_str()])
                    .set(0.0);
                if let Some(dt) = self
                    .last_pcie_sample
                    .get(&idx)
                    .map(|ts| now.saturating_duration_since(*ts).as_secs_f64())
                {
                    if dt > 0.0 {
                        let mut last_tx_kb: Option<u32> = None;
                        let mut last_rx_kb: Option<u32> = None;
                        if let Ok(tx_kb) = device.pcie_throughput(PcieUtilCounter::Send) {
                            last_tx_kb = Some(tx_kb);
                            let delta = (tx_kb as f64 * 1024.0 * dt) as u64;
                            metrics
                                .gpu_pcie_tx_bytes_total
                                .with_label_values(&[uuid_label, gpu_label.as_str()])
                                .inc_by(delta);
                        }
                        if let Ok(rx_kb) = device.pcie_throughput(PcieUtilCounter::Receive) {
                            last_rx_kb = Some(rx_kb);
                            let delta = (rx_kb as f64 * 1024.0 * dt) as u64;
                            metrics
                                .gpu_pcie_rx_bytes_total
                                .with_label_values(&[uuid_label, gpu_label.as_str()])
                                .inc_by(delta);
                        }

                        // Estimate bandwidth percent if we have throughput + link info
                        if let (Some(tx_kb), Some(rx_kb)) = (last_tx_kb, last_rx_kb) {
                            if let (Ok(max_speed), Ok(width)) = (
                                device.pcie_link_max_speed(),
                                device.current_pcie_link_width(),
                            ) {
                                let bytes_per_s = ((tx_kb + rx_kb) as f64) * 1024.0;
                                let lane_budget_bytes =
                                    pcie_lane_bytes_per_sec(max_speed) * (width as f64).max(1.0);
                                if lane_budget_bytes > 0.0 {
                                    let pct = (bytes_per_s / lane_budget_bytes).min(1.0) * 100.0;
                                    metrics
                                        .pcie_bandwidth_percent
                                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                                        .set(pct);
                                }
                            }
                        }
                    }
                }
                self.last_pcie_sample.insert(idx, now);
                metrics
                    .gpu_pcie_tx_bytes_total
                    .with_label_values(&[uuid_label, gpu_label.as_str()])
                    .inc_by(0);
                metrics
                    .gpu_pcie_rx_bytes_total
                    .with_label_values(&[uuid_label, gpu_label.as_str()])
                    .inc_by(0);
                // NvLink utilization/errors (best effort)
                let mut fabric_links: Vec<FabricLink> = Vec::new();
                for link_idx in 0..6u32 {
                    let mut link = device.link_wrapper_for(link_idx);
                    if !link.is_active().unwrap_or(false) {
                        continue;
                    }
                    let link_label = link_idx.to_string();
                    let _ = link.set_utilization_control(
                        NvLinkCounter::One,
                        UtilizationControl {
                            units: UtilizationCountUnit::Bytes,
                            packet_filter: PacketTypes::all(),
                        },
                        false,
                    );
                    if let Ok(util) = link.utilization_counter(NvLinkCounter::One) {
                        let key = (idx, link_idx);
                        let prev = self.nvlink_util_prev.get(&key).copied();
                        if let Some((prev_rx, prev_tx)) = prev {
                            if util.receive >= prev_rx {
                                metrics
                                    .gpu_nvlink_rx_bytes_total
                                    .with_label_values(&[
                                        uuid_label,
                                        gpu_label.as_str(),
                                        link_label.as_str(),
                                    ])
                                    .inc_by(util.receive - prev_rx);
                            }
                            if util.send >= prev_tx {
                                metrics
                                    .gpu_nvlink_tx_bytes_total
                                    .with_label_values(&[
                                        uuid_label,
                                        gpu_label.as_str(),
                                        link_label.as_str(),
                                    ])
                                    .inc_by(util.send - prev_tx);
                            }
                        }
                        let mut rx_delta: Option<u64> = None;
                        let mut tx_delta: Option<u64> = None;
                        if let Some((prev_rx, prev_tx)) = prev {
                            rx_delta = Some(util.receive.saturating_sub(prev_rx));
                            tx_delta = Some(util.send.saturating_sub(prev_tx));
                        }
                        self.nvlink_util_prev.insert(key, (util.receive, util.send));
                        if rx_delta.is_some() || tx_delta.is_some() {
                            fabric_links.push(FabricLink {
                                link: link_idx,
                                link_type: FabricLinkType::NvLink,
                                rx_bytes: rx_delta,
                                tx_bytes: tx_delta,
                                errors: None,
                            });
                        }
                    }

                    for (counter, label) in [
                        (NvLinkErrorCounter::DlReplay, "dl_replay"),
                        (NvLinkErrorCounter::DlRecovery, "dl_recovery"),
                        (NvLinkErrorCounter::DlCrcFlit, "dl_crc_flit"),
                        (NvLinkErrorCounter::DlCrcData, "dl_crc_data"),
                    ] {
                        if let Ok(val) = link.error_counter(counter) {
                            let key = (idx, link_idx, label.to_string());
                            let prev = self.nvlink_err_prev.get(&key).copied().unwrap_or(0);
                            if val >= prev {
                                metrics
                                    .gpu_nvlink_errors_total
                                    .with_label_values(&[
                                        uuid_label,
                                        gpu_label.as_str(),
                                        link_label.as_str(),
                                    ])
                                    .inc_by(val - prev);
                            }
                            self.nvlink_err_prev.insert(key, val);
                            if let Some(f) = fabric_links.iter_mut().find(|f| f.link == link_idx) {
                                let prev_err = f.errors.unwrap_or(0);
                                f.errors = Some(prev_err + val.saturating_sub(prev_err));
                            }
                        }
                    }
                }
                if !fabric_links.is_empty() {
                    status.fabric_links = Some(fabric_links);
                }

                // nvml-wrapper 0.9 exposes replay counter but not uncorrectable PCIe errors
                metrics
                    .gpu_pcie_uncorrectable_errors_total
                    .with_label_values(&[uuid_label, gpu_label.as_str()])
                    .inc_by(0);
                metrics
                    .pcie_link_width
                    .with_label_values(&[uuid_label, gpu_label.as_str()])
                    .set(device.current_pcie_link_width().unwrap_or(0) as f64);
                metrics
                    .pcie_link_gen
                    .with_label_values(&[uuid_label, gpu_label.as_str()])
                    .set(device.current_pcie_link_gen().unwrap_or(0) as f64);
                if let Ok(replay) = device.pcie_replay_counter() {
                    let prev = self.last_pcie_replay.get(&idx).copied().unwrap_or(0);
                    if replay >= prev {
                        metrics
                            .gpu_pcie_replay_errors_total
                            .with_label_values(&[uuid_label, gpu_label.as_str()])
                            .inc_by((replay - prev) as u64);
                    }
                    self.last_pcie_replay.insert(idx, replay);
                } else {
                    metrics
                        .gpu_pcie_replay_errors_total
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .inc_by(0);
                }
                // nvml-wrapper 0.9 does not expose PCIe uncorrectable errors; keep series present.
                if self.enable_mig {
                    #[cfg(all(feature = "gpu-nvml-ffi", feature = "gpu"))]
                    {
                        if let Ok(migs) = collect_mig_devices(nvml, &device) {
                            metrics
                                .gpu_mig_enabled
                                .with_label_values(&[uuid_label, gpu_label.as_str()])
                                .set(if migs.enabled { 1.0 } else { 0.0 });
                            // GI/CI info gauges
                            for gi in &migs.gpu_instances {
                                metrics
                                    .mig_gpu_instance_info
                                    .with_label_values(&[
                                        uuid_label,
                                        gpu_label.as_str(),
                                        gi.id.to_string().as_str(),
                                        gi.profile_id
                                            .map(|p| p.to_string())
                                            .unwrap_or_default()
                                            .as_str(),
                                        gi.placement.as_deref().unwrap_or(""),
                                    ])
                                    .set(1.0);
                            }
                            for ci in &migs.compute_instances {
                                metrics
                                    .mig_compute_instance_info
                                    .with_label_values(&[
                                        uuid_label,
                                        gpu_label.as_str(),
                                        ci.gpu_instance_id.to_string().as_str(),
                                        ci.id.to_string().as_str(),
                                        ci.profile_id
                                            .map(|p| p.to_string())
                                            .unwrap_or_default()
                                            .as_str(),
                                        ci.eng_profile_id
                                            .map(|p| p.to_string())
                                            .unwrap_or_default()
                                            .as_str(),
                                        ci.placement.as_deref().unwrap_or(""),
                                    ])
                                    .set(1.0);
                            }
                            for mig in &migs.devices {
                                let mig_label = mig.uuid.as_deref().unwrap_or(mig.id.to_string().as_str());
                                let compat_label = if self.k8s_mode {
                                    k8s_resource_name(
                                        self.resource_prefix,
                                        mig.profile.as_deref().or(Some("generic")),
                                    )
                                } else {
                                    mig_label.to_string()
                                };
                                if let Some(util) = mig.util_percent {
                                    metrics
                                        .mig_utilization_percent
                                        .with_label_values(&[
                                            uuid_label,
                                            gpu_label.as_str(),
                                            mig_label,
                                        ])
                                        .set(util as f64);
                                    if self.k8s_mode {
                                        metrics
                                            .mig_utilization_percent
                                            .with_label_values(&[
                                                uuid_label,
                                                gpu_label.as_str(),
                                                compat_label.as_str(),
                                            ])
                                            .set(util as f64);
                                    }
                                }
                                if let Some(total) = mig.memory_total_bytes {
                                    metrics
                                        .mig_memory_total_bytes
                                        .with_label_values(&[
                                            uuid_label,
                                            gpu_label.as_str(),
                                            mig_label,
                                        ])
                                        .set(total as f64);
                                    if self.k8s_mode {
                                        metrics
                                            .mig_memory_total_bytes
                                            .with_label_values(&[
                                                uuid_label,
                                                gpu_label.as_str(),
                                                compat_label.as_str(),
                                            ])
                                            .set(total as f64);
                                    }
                                }
                                if let Some(used) = mig.memory_used_bytes {
                                    metrics
                                        .mig_memory_used_bytes
                                        .with_label_values(&[
                                            uuid_label,
                                            gpu_label.as_str(),
                                            mig_label,
                                        ])
                                        .set(used as f64);
                                    if self.k8s_mode {
                                        metrics
                                            .mig_memory_used_bytes
                                            .with_label_values(&[
                                                uuid_label,
                                                gpu_label.as_str(),
                                                compat_label.as_str(),
                                            ])
                                            .set(used as f64);
                                    }
                                }
                                if let Some(sm) = mig.sm_count {
                                    metrics
                                        .mig_sm_count
                                        .with_label_values(&[
                                            uuid_label,
                                            gpu_label.as_str(),
                                            mig_label,
                                        ])
                                        .set(sm as f64);
                                    if self.k8s_mode {
                                        metrics
                                            .mig_sm_count
                                            .with_label_values(&[
                                                uuid_label,
                                                gpu_label.as_str(),
                                                compat_label.as_str(),
                                            ])
                                            .set(sm as f64);
                                    }
                                }
                                // Best-effort per-MIG ECC and BAR1 info using MIG device handle
                                if let Ok(corrected) =
                                    mig.device.total_ecc_errors(MemoryError::Corrected, EccCounter::Volatile)
                                {
                                    metrics
                                        .mig_ecc_corrected_total
                                        .with_label_values(&[
                                            uuid_label,
                                            gpu_label.as_str(),
                                            mig_label,
                                        ])
                                        .inc_by(corrected);
                                    if self.k8s_mode {
                                        metrics
                                            .mig_ecc_corrected_total
                                            .with_label_values(&[
                                                uuid_label,
                                                gpu_label.as_str(),
                                                compat_label.as_str(),
                                            ])
                                            .inc_by(corrected);
                                    }
                                }
                                if let Ok(uncorrected) = mig
                                    .device
                                    .total_ecc_errors(MemoryError::Uncorrected, EccCounter::Volatile)
                                {
                                    metrics
                                        .mig_ecc_uncorrected_total
                                        .with_label_values(&[
                                            uuid_label,
                                            gpu_label.as_str(),
                                            mig_label,
                                        ])
                                        .inc_by(uncorrected);
                                    if self.k8s_mode {
                                        metrics
                                            .mig_ecc_uncorrected_total
                                            .with_label_values(&[
                                                uuid_label,
                                                gpu_label.as_str(),
                                                compat_label.as_str(),
                                            ])
                                            .inc_by(uncorrected);
                                    }
                                }
                                if let Ok(bar1) = mig.device.bar1_memory_info() {
                                    metrics
                                        .mig_bar1_total_bytes
                                        .with_label_values(&[
                                            uuid_label,
                                            gpu_label.as_str(),
                                            mig_label,
                                        ])
                                        .set(bar1.total as f64);
                                    metrics
                                        .mig_bar1_used_bytes
                                        .with_label_values(&[
                                            uuid_label,
                                            gpu_label.as_str(),
                                            mig_label,
                                        ])
                                        .set(bar1.used as f64);
                                    if self.k8s_mode {
                                        metrics
                                            .mig_bar1_total_bytes
                                            .with_label_values(&[
                                                uuid_label,
                                                gpu_label.as_str(),
                                                compat_label.as_str(),
                                            ])
                                            .set(bar1.total as f64);
                                        metrics
                                            .mig_bar1_used_bytes
                                            .with_label_values(&[
                                                uuid_label,
                                                gpu_label.as_str(),
                                                compat_label.as_str(),
                                            ])
                                            .set(bar1.used as f64);
                                    }
                                }
                                metrics
                                    .mig_info
                                    .with_label_values(&[
                                        uuid_label,
                                        gpu_label.as_str(),
                                        mig_label,
                                        mig.profile.as_deref().unwrap_or(""),
                                        mig.placement.as_deref().unwrap_or(""),
                                    ])
                                    .set(1.0);
                            }
                            status.mig_tree = Some(migs);
                            metrics
                                .gpu_mig_supported
                                .with_label_values(&[uuid_label, gpu_label.as_str()])
                                .set(if migs.supported { 1.0 } else { 0.0 });
                        }
                    }
                    #[cfg(not(all(feature = "gpu-nvml-ffi", feature = "gpu")))]
                    {
                        metrics
                            .gpu_mig_supported
                            .with_label_values(&[uuid_label, gpu_label.as_str()])
                            .set(0.0);
                        metrics
                            .gpu_mig_enabled
                            .with_label_values(&[uuid_label, gpu_label.as_str()])
                            .set(0.0);
                    }
                } else {
                    metrics
                        .gpu_mig_supported
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(0.0);
                    metrics
                        .gpu_mig_enabled
                        .with_label_values(&[uuid_label, gpu_label.as_str()])
                        .set(0.0);
                }

                status.health = Some(health.clone());
                statuses.push(status);
            }

            #[cfg(target_os = "linux")]
            {
                if let Some(es) = event_set.as_ref() {
                    // Drain a few events without blocking long; we rely on periodic scrapes.
                    for _ in 0..32 {
                        match es.wait(0) {
                            Ok(ev) => {
                            let ev_uuid =
                                ev.device.uuid().unwrap_or_else(|_| "unknown".to_string());
                            let index_label = uuid_to_index
                                .get(&ev_uuid)
                                .cloned()
                                .unwrap_or_else(|| "unknown".to_string());
                            let event =
                                if ev.event_type.contains(EventTypes::CRITICAL_XID_ERROR) {
                                    "xid"
                                } else if ev.event_type.contains(EventTypes::SINGLE_BIT_ECC_ERROR)
                                {
                                    "ecc_single"
                                } else if ev.event_type.contains(EventTypes::DOUBLE_BIT_ECC_ERROR)
                                {
                                    "ecc_double"
                                } else if ev.event_type.contains(EventTypes::PSTATE_CHANGE) {
                                    "pstate"
                                } else if ev.event_type.contains(EventTypes::CLOCK_CHANGE) {
                                    "clock"
                                } else {
                                    "other"
                                };
                            let labels = &[ev_uuid.as_str(), index_label.as_str(), event];
                            metrics.gpu_events_total.with_label_values(labels).inc();
                            if event == "xid" {
                                metrics.gpu_xid_errors_total.with_label_values(labels).inc();
                                // record last XID in health if we tracked mapping
                            }
                        }
                            Err(NvmlError::Timeout) => break,
                            Err(_) => break,
                        }
                    }
                }
            }

            self.status.set_gpu_statuses(statuses);
        }

        // If GPU feature is disabled, collection is a no-op.
        Ok(())
    }
}

#[cfg(feature = "gpu")]
fn set_throttle_metric(vec: &GaugeVec, uuid: &str, index: &str, reason: &str, active: bool) {
    vec.with_label_values(&[uuid, index, reason])
        .set(if active { 1.0 } else { 0.0 });
}

#[cfg(feature = "gpu")]
fn k8s_resource_name(prefix: &str, mig_profile: Option<&str>) -> String {
    if let Some(profile) = mig_profile {
        format!("{}/mig-{}", prefix, profile.replace('.', "-"))
    } else {
        format!("{}/gpu", prefix)
    }
}

#[cfg(feature = "gpu")]
fn pcie_lane_bytes_per_sec(speed: PcieLinkMaxSpeed) -> f64 {
    match speed {
        PcieLinkMaxSpeed::MegabytesPerSecond2500 => 2_500_000.0 * 1_000.0,
        PcieLinkMaxSpeed::MegabytesPerSecond5000 => 5_000_000.0 * 1_000.0,
        PcieLinkMaxSpeed::MegabytesPerSecond8000 => 8_000_000.0 * 1_000.0,
        PcieLinkMaxSpeed::MegabytesPerSecond16000 => 16_000_000.0 * 1_000.0,
        PcieLinkMaxSpeed::MegabytesPerSecond32000 => 32_000_000.0 * 1_000.0,
        _ => 0.0,
    }
}

#[cfg(feature = "gpu")]
fn build_filter(raw: Option<&str>) -> Option<HashSet<String>> {
    raw.filter(|s| !s.is_empty() && *s != "all")
        .map(|s| {
            s.split(',')
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect()
        })
}

#[cfg(all(feature = "gpu", feature = "gpu-nvml-ffi"))]
fn collect_mig_devices(
    nvml: &Nvml,
    parent: &nvml_wrapper::Device,
) -> Result<MigTree> {
    use std::os::raw::{c_int, c_uint};
    let mut current_mode: c_uint = 0;
    let mut pending: c_uint = 0;
    let parent_handle = unsafe { parent.handle() };
    let mig_mode_res = unsafe { nvmlDeviceGetMigMode(parent_handle, &mut current_mode, &mut pending) };
    let supported = mig_mode_res == nvml_wrapper_sys::bindings::nvmlReturn_enum_NVML_SUCCESS;
    if !supported {
        return Ok(MigTree {
            supported: false,
            enabled: false,
            gpu_instances: Vec::new(),
            compute_instances: Vec::new(),
            devices: Vec::new(),
        });
    }
    let enabled = current_mode == 1;
    let mut max_count: c_uint = 0;
    unsafe {
        nvmlDeviceGetMaxMigDeviceCount(parent_handle, &mut max_count);
    }
    let mut devices = Vec::new();
    let mut gi_map: HashMap<u32, GpuInstanceNode> = HashMap::new();
    let mut ci_nodes: Vec<ComputeInstanceNode> = Vec::new();
    for idx in 0..max_count {
        let mut mig_handle = std::ptr::null_mut();
        let res = unsafe { nvmlDeviceGetMigDeviceHandleByIndex(parent_handle, idx, &mut mig_handle) };
        if res != nvml_wrapper_sys::bindings::nvmlReturn_enum_NVML_SUCCESS {
            continue;
        }
        // Obtain full device handle for MIG to use safe wrapper methods where possible.
        let mut full_handle = std::ptr::null_mut();
        let _ = unsafe { nvmlDeviceGetDeviceHandleFromMigDeviceHandle(mig_handle, &mut full_handle) };
        let handle_to_use = if !full_handle.is_null() { full_handle } else { mig_handle };
        let mig_device = unsafe { nvml_wrapper::Device::new(handle_to_use, nvml) };
        let mig_uuid = mig_device.uuid().ok();
        let mem_info = mig_device.memory_info().ok();
        let util = mig_device.utilization_rates().ok();
        let sm_count = mig_device.multi_processor_count().ok();
        let mut gi_id: c_uint = 0;
        let mut ci_id: c_uint = 0;
        let _ = unsafe { nvml_wrapper_sys::bindings::nvmlDeviceGetGpuInstanceId(mig_handle, &mut gi_id) };
        let _ = unsafe { nvml_wrapper_sys::bindings::nvmlDeviceGetComputeInstanceId(mig_handle, &mut ci_id) };
        // Populate GI info best-effort
        if gi_id > 0 && !gi_map.contains_key(&gi_id) {
            let mut gi_handle = std::ptr::null_mut();
            if unsafe {
                nvml_wrapper_sys::bindings::nvmlDeviceGetGpuInstanceById(
                    parent_handle,
                    gi_id,
                    &mut gi_handle,
                )
            } == nvml_wrapper_sys::bindings::nvmlReturn_enum_NVML_SUCCESS
            {
                let mut gi_info: nvml_wrapper_sys::bindings::nvmlGpuInstanceInfo_t =
                    unsafe { std::mem::zeroed() };
                let _ = unsafe { nvml_wrapper_sys::bindings::nvmlGpuInstanceGetInfo(gi_handle, &mut gi_info) };
                let placement = Some(format!(
                    "{}:slice{}",
                    gi_info.placement.sliceId, gi_info.placement.instanceSliceId
                ));
                gi_map.insert(
                    gi_id,
                    GpuInstanceNode {
                        id: gi_id,
                        profile_id: Some(gi_info.profileId),
                        placement,
                    },
                );
                if ci_id > 0 {
                    let mut ci_handle = std::ptr::null_mut();
                    if unsafe {
                        nvml_wrapper_sys::bindings::nvmlGpuInstanceGetComputeInstanceById(
                            gi_handle, ci_id, &mut ci_handle,
                        )
                    } == nvml_wrapper_sys::bindings::nvmlReturn_enum_NVML_SUCCESS
                    {
                        let mut ci_info: nvml_wrapper_sys::bindings::nvmlComputeInstanceInfo_t =
                            unsafe { std::mem::zeroed() };
                        let _ = unsafe {
                            nvml_wrapper_sys::bindings::nvmlComputeInstanceGetInfo(
                                ci_handle,
                                &mut ci_info,
                            )
                        };
                        ci_nodes.push(ComputeInstanceNode {
                            gpu_instance_id: gi_id,
                            id: ci_id,
                            profile_id: Some(ci_info.profileId),
                            eng_profile_id: Some(ci_info.engineProfile),
                            placement: Some(format!(
                                "{}:slice{}",
                                ci_info.placement.sliceId, ci_info.placement.instanceSliceId
                            )),
                        });
                    }
                }
            }
        }
        let mig_id = format!("mig{}", idx);
        let placement_str = gi_map
            .get(&gi_id)
            .and_then(|g| g.placement.clone())
            .unwrap_or_else(|| format!("gi{}", gi_id));
        let profile_str = gi_map
            .get(&gi_id)
            .and_then(|g| g.profile_id)
            .map(|p| p.to_string());
        devices.push(MigDeviceStatus {
            id: mig_uuid.clone().unwrap_or(mig_id.clone()),
            uuid: mig_uuid,
            memory_total_bytes: mem_info.as_ref().map(|m| m.total),
            memory_used_bytes: mem_info.map(|m| m.used),
            util_percent: util.map(|u| u.gpu as u32),
            sm_count,
            profile: profile_str,
            placement: Some(placement_str),
        });
    }

    Ok(MigTree {
        supported,
        enabled,
        gpu_instances: gi_map.values().cloned().collect(),
        compute_instances: ci_nodes,
        devices,
    })
}
