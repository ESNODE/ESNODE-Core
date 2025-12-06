# ESNODE | Source Available BUSL-1.1 | Copyright (c) 2024 Estimatedstocks AB

# ESNODE-Core

This repository contains the source, build tooling, and documentation for the ESNODE-Core Agent. The ESNODE-Pulse controller is maintained separately (licensed) and is not part of this codebase.
## Supported server OS targets (AI infra focused)
- Ubuntu Server (primary, ~60–70% of AI fleet; best CUDA/driver/toolchain support)
- RHEL-compatible: RHEL / Rocky Linux / AlmaLinux (enterprise compliance, FIPS-ready)
- NVIDIA DGX OS (Ubuntu-based, pre-tuned for DGX appliances)
- SUSE Linux Enterprise Server (SLES) (enterprise/HPC niches)
- Debian (research/custom environments)

Packaging is provided as tar.gz plus optional deb/rpm; Windows zips are included for hybrid labs. On Linux, install scripts set up binaries under `/usr/local/bin` and create systemd units so `esnode-core` runs without extra PATH tweaks (ESNODE-Pulse ships separately).

ESNODE-Core is a GPU-aware host metrics exporter for Linux nodes. It exposes CPU, memory, disk, network, and GPU telemetry at `/metrics` in Prometheus text format so observability stacks can scrape a single endpoint per node. Agents can run standalone or attach to an ESNODE-Pulse control plane while keeping Prometheus/OTLP outputs unchanged.

## Features (v0.1 scope)
- Single binary with zero-config defaults (`0.0.0.0:9100`, 5s interval).
- Collectors: CPU, memory, disk, network, GPU (NVML-based; gracefully disabled if unavailable).
- Power-aware: optional power collector reads RAPL/hwmon/BMC paths for CPU/package/node power; GPU power via NVML.
- Self-observability: scrape duration + error counters per collector.
- Health endpoint at `/healthz`.
- JSON status endpoint at `/status` (`/v1/status` alias) with node load, power, temps, GPU snapshot, last scrape/errors; SSE stream at `/events` for near-real-time loops.

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
- `esnode-pulse`: licensed controller that polls agents and centralizes policy/alerts (not included in this repository).

See `docs/architecture.md` and `docs/platform-matrix.md` for topology and build targets.

## Build & Run
```bash
cargo build --workspace --release
./target/release/esnode-core
```

Configuration precedence: CLI flags > env vars > `esnode.toml` > defaults. See `docs/quickstart.md` for full examples.

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
