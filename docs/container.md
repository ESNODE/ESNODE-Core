# Container images for ESNODE-Core

## Minimal distroless image (Phase 1)
- Dockerfile: `deploy/docker/Dockerfile.distroless`
- Runtime: `gcr.io/distroless/static:nonroot`, non-root user, exposed port `9100`, no init/systemd.
- Expected input: a prebuilt static (musl) binary named `esnode-core` in the build context (or point `ESNODE_BINARY` to your binary path).

### Build (single arch)
```bash
# Build a static binary first (example for amd64):
cargo build --release --locked --target x86_64-unknown-linux-musl
# Copy/rename into build context root as esnode-core, then:
docker build -f deploy/docker/Dockerfile.distroless -t esnode-core:local .
```

### Build multi-arch (amd64 + arm64) with buildx
```bash
# Prepare per-arch binaries before building:
#   target/x86_64-unknown-linux-musl/release/esnode-core
#   target/aarch64-unknown-linux-musl/release/esnode-core
docker buildx create --use --name esnode-builder || true
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -f deploy/docker/Dockerfile.distroless \
  -t ghcr.io/ESNODE/esnode-core:0.1.0 \
  -t ghcr.io/ESNODE/esnode-core:latest \
  --push .
```

> Note: distroless expects static binaries; ensure NVML/libnvidia-ml.so is available via the NVIDIA runtime/host drivers when running with GPU access.

### Run (basic)
```bash
docker run --rm --net=host --pid=host ghcr.io/ESNODE/esnode-core:0.1.0
```

Adjust mounts/privileges for GPU telemetry (e.g., NVIDIA Container Toolkit) and `/sys` access as needed.
