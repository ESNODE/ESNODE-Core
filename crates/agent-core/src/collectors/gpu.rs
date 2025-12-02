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
use prometheus::GaugeVec;
use std::collections::HashMap;
#[cfg(feature = "gpu")]
use std::time::Instant;

use crate::collectors::Collector;
use crate::metrics::MetricsRegistry;
use crate::state::{GpuStatus, StatusState};

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
    warned_mig: bool,
    status: StatusState,
}

impl GpuCollector {
    pub fn new(status: StatusState) -> (Self, Option<String>) {
        #[cfg(feature = "gpu")]
        {
            match Nvml::init() {
                Ok(nvml) => (
                    Self {
                        nvml: Some(nvml),
                        ecc_prev: HashMap::new(),
                        last_power: HashMap::new(),
                        last_pcie_sample: HashMap::new(),
                        last_pcie_replay: HashMap::new(),
                        nvlink_util_prev: HashMap::new(),
                        nvlink_err_prev: HashMap::new(),
                        warned_mig: false,
                        status,
                    },
                    None,
                ),
                Err(e) => (
                    Self {
                        nvml: None,
                        ecc_prev: HashMap::new(),
                        last_power: HashMap::new(),
                        last_pcie_sample: HashMap::new(),
                        last_pcie_replay: HashMap::new(),
                        nvlink_util_prev: HashMap::new(),
                        nvlink_err_prev: HashMap::new(),
                        warned_mig: false,
                        status,
                    },
                    Some(format!("GPU collector disabled: {}", e)),
                ),
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
            for idx in 0..count {
                let device = nvml.device_by_index(idx)?;
                let gpu_label = idx.to_string();
                let now = Instant::now();
                metrics
                    .pcie_bandwidth_percent
                    .with_label_values(&[gpu_label.as_str()])
                    .set(0.0);
                let mut status = GpuStatus {
                    gpu: gpu_label.clone(),
                    ..Default::default()
                };

                if let Ok(util) = device.utilization_rates() {
                    metrics
                        .gpu_utilization_percent
                        .with_label_values(&[gpu_label.as_str()])
                        .set(util.gpu as f64);
                    status.util_percent = Some(util.gpu as f64);
                }

                if let Ok(memory) = device.memory_info() {
                    metrics
                        .gpu_memory_total_bytes
                        .with_label_values(&[gpu_label.as_str()])
                        .set(memory.total as f64);
                    metrics
                        .gpu_memory_used_bytes
                        .with_label_values(&[gpu_label.as_str()])
                        .set(memory.used as f64);
                    status.memory_total_bytes = Some(memory.total as f64);
                    status.memory_used_bytes = Some(memory.used as f64);
                }

                if let Ok(temp) = device.temperature(TemperatureSensor::Gpu) {
                    metrics
                        .gpu_temperature_celsius
                        .with_label_values(&[gpu_label.as_str()])
                        .set(temp as f64);
                    status.temperature_celsius = Some(temp as f64);
                }

                if let Ok(power) = device.power_usage() {
                    metrics
                        .gpu_power_watts
                        .with_label_values(&[gpu_label.as_str()])
                        .set(power as f64 / 1000.0);
                    let watts = power as f64 / 1000.0;
                    status.power_watts = Some(watts);
                    if let Some((prev_watts, ts)) = self.last_power.get(&idx) {
                        let dt = now.saturating_duration_since(*ts).as_secs_f64();
                        if dt > 0.0 {
                            let energy = (prev_watts * dt).floor() as u64;
                            metrics
                                .gpu_energy_joules_total
                                .with_label_values(&[gpu_label.as_str()])
                                .inc_by(energy);
                        }
                    }
                    self.last_power.insert(idx, (watts, now));
                }

                if let Ok(limit) = device.power_management_limit() {
                    metrics
                        .gpu_power_limit_watts
                        .with_label_values(&[gpu_label.as_str()])
                        .set(limit as f64 / 1000.0);
                }

                if let Ok(fan) = device.fan_speed(0) {
                    metrics
                        .gpu_fan_speed_percent
                        .with_label_values(&[gpu_label.as_str()])
                        .set(fan as f64);
                    status.fan_percent = Some(fan as f64);
                }

                if let Ok(sm_clock) = device.clock_info(Clock::SM) {
                    metrics
                        .gpu_clock_sm_mhz
                        .with_label_values(&[gpu_label.as_str()])
                        .set(sm_clock as f64);
                    status.clock_sm_mhz = Some(sm_clock as f64);
                }

                if let Ok(mem_clock) = device.clock_info(Clock::Memory) {
                    metrics
                        .gpu_clock_mem_mhz
                        .with_label_values(&[gpu_label.as_str()])
                        .set(mem_clock as f64);
                    status.clock_mem_mhz = Some(mem_clock as f64);
                }

                if let Ok(gfx_clock) = device.clock_info(Clock::Graphics) {
                    metrics
                        .gpu_clock_graphics_mhz
                        .with_label_values(&[gpu_label.as_str()])
                        .set(gfx_clock as f64);
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
                                .with_label_values(&[gpu_label.as_str(), label])
                                .inc_by(total - prev);
                        }
                        self.ecc_prev.insert(key, total);
                    } else {
                        // keep series visible even if call is unsupported
                        metrics
                            .gpu_ecc_errors_total
                            .with_label_values(&[gpu_label.as_str(), label])
                            .inc_by(0);
                    }
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
                        gpu_label.as_str(),
                        "thermal",
                        thermal,
                    );
                    set_throttle_metric(
                        &metrics.gpu_throttle_reason,
                        gpu_label.as_str(),
                        "power",
                        power,
                    );
                    set_throttle_metric(
                        &metrics.gpu_throttle_reason,
                        gpu_label.as_str(),
                        "other",
                        !(thermal || power),
                    );
                } else {
                    set_throttle_metric(
                        &metrics.gpu_throttle_reason,
                        gpu_label.as_str(),
                        "thermal",
                        false,
                    );
                    set_throttle_metric(
                        &metrics.gpu_throttle_reason,
                        gpu_label.as_str(),
                        "power",
                        false,
                    );
                    set_throttle_metric(
                        &metrics.gpu_throttle_reason,
                        gpu_label.as_str(),
                        "other",
                        false,
                    );
                }

                // Initialize always-on counters for compatibility.
                metrics
                    .gpu_energy_joules_total
                    .with_label_values(&[gpu_label.as_str()])
                    .inc_by(0);
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
                                .with_label_values(&[gpu_label.as_str()])
                                .inc_by(delta);
                        }
                        if let Ok(rx_kb) = device.pcie_throughput(PcieUtilCounter::Receive) {
                            last_rx_kb = Some(rx_kb);
                            let delta = (rx_kb as f64 * 1024.0 * dt) as u64;
                            metrics
                                .gpu_pcie_rx_bytes_total
                                .with_label_values(&[gpu_label.as_str()])
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
                                        .with_label_values(&[gpu_label.as_str()])
                                        .set(pct);
                                }
                            }
                        }
                    }
                }
                self.last_pcie_sample.insert(idx, now);
                metrics
                    .gpu_pcie_tx_bytes_total
                    .with_label_values(&[gpu_label.as_str()])
                    .inc_by(0);
                metrics
                    .gpu_pcie_rx_bytes_total
                    .with_label_values(&[gpu_label.as_str()])
                    .inc_by(0);
                // NvLink utilization/errors (best effort)
                for link_idx in 0..6u32 {
                    let mut link = device.link_wrapper_for(link_idx);
                    if link.is_active().unwrap_or(false) {
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
                                            gpu_label.as_str(),
                                            link_label.as_str(),
                                        ])
                                        .inc_by(util.receive - prev_rx);
                                }
                                if util.send >= prev_tx {
                                    metrics
                                        .gpu_nvlink_tx_bytes_total
                                        .with_label_values(&[
                                            gpu_label.as_str(),
                                            link_label.as_str(),
                                        ])
                                        .inc_by(util.send - prev_tx);
                                }
                            }
                            self.nvlink_util_prev.insert(key, (util.receive, util.send));
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
                                            gpu_label.as_str(),
                                            link_label.as_str(),
                                        ])
                                        .inc_by(val - prev);
                                }
                                self.nvlink_err_prev.insert(key, val);
                            }
                        }
                    }
                }

                // nvml-wrapper 0.9 exposes replay counter but not uncorrectable PCIe errors
                metrics
                    .gpu_pcie_uncorrectable_errors_total
                    .with_label_values(&[gpu_label.as_str()])
                    .inc_by(0);
                metrics
                    .pcie_link_width
                    .with_label_values(&[gpu_label.as_str()])
                    .set(device.current_pcie_link_width().unwrap_or(0) as f64);
                metrics
                    .pcie_link_gen
                    .with_label_values(&[gpu_label.as_str()])
                    .set(device.current_pcie_link_gen().unwrap_or(0) as f64);
                if let Ok(replay) = device.pcie_replay_counter() {
                    let prev = self.last_pcie_replay.get(&idx).copied().unwrap_or(0);
                    if replay >= prev {
                        metrics
                            .gpu_pcie_replay_errors_total
                            .with_label_values(&[gpu_label.as_str()])
                            .inc_by((replay - prev) as u64);
                    }
                    self.last_pcie_replay.insert(idx, replay);
                } else {
                    metrics
                        .gpu_pcie_replay_errors_total
                        .with_label_values(&[gpu_label.as_str()])
                        .inc_by(0);
                }
                // nvml-wrapper 0.9 does not expose PCIe uncorrectable errors; keep series present.
                metrics
                    .mig_utilization_percent
                    .with_label_values(&[gpu_label.as_str(), "0"])
                    .set(0.0);
                metrics
                    .mig_memory_total_bytes
                    .with_label_values(&[gpu_label.as_str(), "0"])
                    .set(0.0);
                metrics
                    .mig_memory_used_bytes
                    .with_label_values(&[gpu_label.as_str(), "0"])
                    .set(0.0);
                metrics
                    .mig_sm_count
                    .with_label_values(&[gpu_label.as_str(), "0"])
                    .set(0.0);
                metrics
                    .mig_energy_joules_total
                    .with_label_values(&[gpu_label.as_str(), "0"])
                    .inc_by(0);
                metrics
                    .gpu_mig_supported
                    .with_label_values(&[gpu_label.as_str()])
                    .set(0.0);
                if !self.warned_mig {
                    tracing::warn!("MIG metrics not supported with current NVML bindings; metrics will remain zero.");
                    self.warned_mig = true;
                }

                statuses.push(status);
            }

            self.status.set_gpu_statuses(statuses);
        }

        // If GPU feature is disabled, collection is a no-op.
        Ok(())
    }
}

#[cfg(feature = "gpu")]
fn set_throttle_metric(vec: &GaugeVec, gpu: &str, reason: &str, active: bool) {
    vec.with_label_values(&[gpu, reason])
        .set(if active { 1.0 } else { 0.0 });
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
