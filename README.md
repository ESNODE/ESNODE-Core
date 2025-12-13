# ESNODE | Source Available BUSL-1.1 | Copyright (c) 2024 Estimatedstocks AB

# ESNODE-Core

![ESNODE-Core TUI Home Screen](docs/images/esnode-tui-home.png)

This repository contains the source, build tooling, and documentation for the ESNODE-Core Agent. The ESNODE-Pulse controller is maintained separately (licensed) and is not part of this codebase.
## Supported server OS targets (AI infra focused)
- Ubuntu Server (primary, ~60–70% of AI fleet; best CUDA/driver/toolchain support)
- RHEL-compatible: RHEL / Rocky Linux / AlmaLinux (enterprise compliance, FIPS-ready)
- NVIDIA DGX OS (Ubuntu-based, pre-tuned for DGX appliances)
- SUSE Linux Enterprise Server (SLES) (enterprise/HPC niches)
- Debian (research/custom environments)

Packaging is provided as tar.gz plus optional deb/rpm; Windows zips are included for hybrid labs. On Linux, install scripts set up binaries under `/usr/local/bin` and create systemd units so `esnode-core` runs without extra PATH tweaks (ESNODE-Pulse ships separately).

ESNODE-Core is a GPU-aware host metrics exporter for Linux nodes. It exposes CPU, memory, disk, network, and GPU telemetry at `/metrics` in Prometheus text format so observability stacks can scrape a single endpoint per node. Agents can run standalone or attach to an ESNODE-Pulse control plane while keeping Prometheus/OTLP outputs unchanged.

### GPU/MIG visibility and build requirements
- GPU metrics require the `gpu` feature (enabled by default). MIG metrics additionally require building with the `gpu-nvml-ffi` feature and enabling `enable_gpu_mig` in config; otherwise MIG metrics will remain zero.
- `gpu_visible_devices` (or `NVIDIA_VISIBLE_DEVICES`) filters which GPUs are scraped; empty/`all` scrapes all.
- `mig_config_devices` (or `NVIDIA_MIG_CONFIG_DEVICES`) further filters which GPUs are considered for MIG scraping when `enable_gpu_mig` is true.
- `k8s_mode` emits a small set of legacy-compatible GPU metrics with a single `gpu` label (suffix `_compat`) using Kubernetes/CDI resource-style names (`nvidia.com/gpu`, `nvidia.com/gpu-<mig-profile>-mig`) alongside the UUID+index labeled metrics to reduce dashboard breakage.
- MIG compatibility labels are only emitted when `k8s_mode` is enabled; MIG info/metrics still require `gpu-nvml-ffi` + `enable_gpu_mig`.
- `enable_gpu_events` starts an NVML event loop for XID/ECC/power/clock events (best-effort). The loop is non-blocking with a short poll and may miss very bursty streams; counters are monotonic but not guaranteed exact.
- `gpu-nvml-ffi-ext` is an optional feature gate for extra NVML FFI (PCIe field counters, etc.). These are best-effort and unverified without suitable hardware; placeholders remain zero when unsupported.
- NVSwitch/copy-engine clocks/power-cap reason codes are exposed as gauges but rely on NVML support; many remain zero on hardware/driver versions that do not surface them.


## Features (v0.1 scope)
- Single binary with zero-config defaults (`0.0.0.0:9100`, 5s interval).
- Collectors: CPU, memory, disk, network, GPU (NVML-based; gracefully disabled if unavailable).
- Power-aware: optional power collector reads RAPL/hwmon/BMC paths for CPU/package/node power; GPU power via NVML.
- Self-observability: scrape duration + error counters per collector.
- Health endpoint at `/healthz`.
- JSON status endpoint at `/status` (`/v1/status` alias) with node load, power, temps, GPU snapshot (identity/health/MIG tree), last scrape/errors; SSE stream at `/events` for near-real-time loops.

## Power- & Model-awareness (roadmap)
- Power: keep `esnode_gpu_power_watts` and add optional `esnode_cpu_package_power_watts`, `esnode_node_power_watts`, and PDU/BMC readings where available (RAPL/hwmon/IPMI).
 - Model metadata: pair ESNODE with app-exported metrics such as `model_tokens_total`, `model_requests_total`, and latency histograms labeled by `model`/`namespace`.
 - Efficiency: use PromQL like `sum(rate(model_tokens_total[5m])) / sum(rate(esnode_node_power_watts[5m]))` for tokens-per-watt and scale to tokens-per-kWh; see `docs/monitoring-examples.md` for dashboards.

## Licensing & Distribution
- Source-available under ESNODE BUSL-1.1 (see `LICENSE`).
- Trademarks governed by `docs/TRADEMARK_POLICY.md`; no rebranding or redistribution of binaries.
- Contributions require agreement to `docs/CLA.md`.
- Official binaries and commercial terms are controlled solely by Estimatedstocks AB.

### Product split (effective now)
- **ESNODE-Core (public, source-available)**  
  - License: current ESNODE BUSL-style terms.  
  - Usage: free for internal use; redistribution/trademark restrictions still apply.  
  - Distribution: public binaries at `https://esnode.co/downloads`.
- **ESNODE-Pulse (licensed-only, revenue-based; separate repo)**  
  - License: proprietary.  
  - Revenue rule: ≤ USD 2M revenue → free starter license after registration; > USD 2M → paid subscription required before production use.  
  - Distribution: **not** in this repository. Binaries are provided only after registration via `https://esnode.co/products#pulse` (or designated portal).

## Components
- `esnode-core`: per-node collector exposing Prometheus `/metrics`, JSON `/status` (`/v1/status`), and SSE `/events`.
- `esnode-orchestrator`: optional autonomous resource manager (embedded lib, CLI-configurable).
- `esnode-pulse`: licensed controller that polls agents and centralizes policy/alerts (not included in this repository).

See `docs/architecture.md` and `docs/platform-matrix.md` for topology and build targets.

## Build & Run
```bash
cargo build --workspace --release
./target/release/esnode-core
```
- Cross-compiling on macOS for `x86_64-unknown-linux-gnu`/`aarch64-unknown-linux-gnu` requires the corresponding GNU toolchains (e.g., `brew install x86_64-unknown-linux-gnu`).

Configuration precedence: CLI flags > env vars > `esnode.toml` > defaults. See `docs/quickstart.md` for full examples.
- Config flags of interest:
  - `enable_gpu_mig` (default false) – turn on MIG scraping when built with `gpu-nvml-ffi`.
  - `enable_gpu_events` (default false) – run NVML event loop (best-effort) for XID/ECC/clock/power events.
  - `enable_gpu_amd` (default false) – experimental AMD/ROCm collector scaffolding; emits no metrics unless rsmi/rocm-smi support is added.
  - `k8s_mode` (default false) – emit compatibility labels using Kubernetes/CDI resource names alongside UUID/index labels.
  - `gpu_visible_devices` / `NVIDIA_VISIBLE_DEVICES` – filter visible GPUs.
  - `mig_config_devices` / `NVIDIA_MIG_CONFIG_DEVICES` – filter MIG-capable GPUs when `enable_gpu_mig` is true.
  - Optional `gpu-nvml-ffi-ext` feature enables additional NVML field-based counters (PCIe/etc.), best-effort only.

Local TSDB path (default): when `enable_local_tsdb` is true, the agent now resolves `local_tsdb_path` to `$XDG_DATA_HOME/esnode/tsdb` or `~/.local/share/esnode/tsdb` so non-root runs don’t fail on `/var/lib`. Set `ESNODE_LOCAL_TSDB_PATH` or the config key if you want `/var/lib/esnode/tsdb` and ensure the directory is writable by the agent user.

### Releases & GitHub tagging
- Tagging `vX.Y.Z` on the default branch triggers `.github/workflows/release.yml`, which:
  - Runs `cargo test --workspace --locked --target <triple>` on Linux (x86_64), macOS (aarch64), and Windows (x86_64).
  - Builds release binaries with default features for the same triples.
  - Packages artifacts as tar.gz (Linux/macOS) or zip (Windows) and attaches them to the GitHub Release created from the tag.
- Artifact names:
  - `esnode-core-linux-x86_64.tar.gz`
  - `esnode-core-macos-aarch64.tar.gz`
  - `esnode-core-windows-x86_64.zip`
- Binaries are built with default features; MIG metrics still require `gpu-nvml-ffi` and `enable_gpu_mig` when running on MIG-capable hosts.
- For additional targets or feature builds, run `cargo build --release --locked --target <triple>` locally and publish as needed.
- Manual packaging: `scripts/dist/esnode-core-release.sh` (optionally with `ESNODE_VERSION=X.Y.Z`) builds and collects Linux tar/deb/rpm bundles (and Windows zip if toolchain available) under `public/distribution/releases/`. Push a tag `vX.Y.Z` after verification to publish GitHub release artifacts automatically.

Community & policies:
- Contribution guidelines: see `CONTRIBUTING.md`.
- Code of Conduct: see `CODE_OF_CONDUCT.md`.
- Security reporting: see `SECURITY.md`.
- Support & upgrade policy: see `docs/support-policy.md`.
- Metric label migration (UUID-first): see `docs/migrations.md`.
- Sponsorship: see `docs/sponsorship.md` (GitHub Sponsors for ESNODE).
- Containers: see `docs/container.md` for distroless build/run instructions.

### Agent ↔ Server connection (summary)
- Standalone: full local CLI/TUI, toggle metric sets, `/metrics` always on.
- Connect to server: `esnode-core server connect --address <host:port> [--token <join_token>]` persists server + IDs and flips to managed mode (local tuning disabled, metrics plane untouched).
- Disconnect: `esnode-core server disconnect` returns to standalone.
- Status: `esnode-core server status` shows server, cluster ID, node ID, last contact.
- TUI: `esnode-core cli` shows full menu when standalone; shows managed read-only panel when attached to ESNODE-Pulse.

## Install packages (Agent – public)
Example commands (adjust version/OS paths):
- Ubuntu/Debian (`.deb`):
  ```bash
  wget -O esnode-core_0.1.0_amd64.deb https://esnode.co/downloads/esnode-core_0.1.0_amd64.deb
  sudo dpkg -i esnode-core_0.1.0_amd64.deb
  sudo systemctl enable esnode-core && sudo systemctl start esnode-core
  ```
- RHEL/CentOS (`.rpm`):
  ```bash
  wget -O esnode-core-0.1.0-1.x86_64.rpm https://esnode.co/downloads/esnode-core-0.1.0-1.x86_64.rpm
  sudo rpm -i esnode-core-0.1.0-1.x86_64.rpm
  sudo systemctl enable esnode-core && sudo systemctl start esnode-core
  ```
- Generic Linux (`tar.gz`):
  ```bash
  wget -O esnode-core-0.1.0-linux-amd64.tar.gz https://esnode.co/downloads/esnode-core-0.1.0-linux-amd64.tar.gz
  tar xvf esnode-core-0.1.0-linux-amd64.tar.gz
  sudo mv esnode-core /usr/local/bin/
  esnode-core --version
  ```
- Windows/macOS artifacts will follow the same public repo layout when available.

## ESNODE-Pulse (Enterprise controller – licensed)
- Not distributed publicly or within this codebase. Binaries are provided only after registration/approval.  
- Revenue rule: ≤ USD 2M revenue → free starter license (registration required). > USD 2M → paid subscription before production use.  
- Request access: `https://esnode.co/products#pulse` (submit company + revenue band, accept terms).  
- ESNODE-Pulse binaries must not be mirrored to the public distribution paths; see the private repository for build details.

## Deployment artifacts
- Docker: `deploy/docker/Dockerfile`
- systemd: `deploy/systemd/esnode-core.service`
- Kubernetes DaemonSet: `deploy/k8s/daemonset.yaml`

## Documentation
- Quickstart: `docs/quickstart.md`
- Metrics reference: `docs/metrics-list.md`
- Monitoring examples: `docs/monitoring-examples.md`
- Architecture: `docs/architecture.md`
- Platform matrix: `docs/platform-matrix.md`
- Smoke test script: `scripts/smoke.sh` (builds, runs core+pulse locally, curls endpoints)
