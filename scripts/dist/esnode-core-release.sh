#!/usr/bin/env bash
set -euo pipefail

# Build and collect ESNODE-Core distributables (Linux tar/deb/rpm; Windows zip if target installed).
# Works from any OS as long as the relevant Rust targets/toolchains are installed.
#
# Usage (from repo root):
#   scripts/dist/esnode-core-release.sh
#   ESNODE_VERSION=1.0.0 scripts/dist/esnode-core-release.sh
#   ESNODE_TARGET=x86_64-unknown-linux-gnu ESNODE_ARCH=amd64 scripts/dist/esnode-core-release.sh
#
# Outputs land under:
#   public/distribution/esnode-core/...   (canonical layout from build-agent.sh)
#   public/distribution/releases/<label>  (flattened copies per target)

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT_DIR}"

version_from_cargo() {
  sed -n 's/^version = "\([^"]*\)"$/\1/p' crates/agent-bin/Cargo.toml | head -n1
}

VERSION="${ESNODE_VERSION:-$(version_from_cargo)}"
if [[ -z "${VERSION}" ]]; then
  echo "ERROR: unable to determine version (set ESNODE_VERSION)" >&2
  exit 1
fi

DEST_ROOT="${ROOT_DIR}/public/distribution/releases"
mkdir -p "${DEST_ROOT}"

call_build() {
  local target="$1"
  local arch="$2"
  local label="$3"

  echo "==> Building ${label} (target=${target:-host}, arch=${arch})"

  # Provide sane linker/arch tools for Linux cross-builds on macOS.
  # Uses Homebrew cross toolchains if present.
  local env=()
  if [[ "${target}" == "x86_64-unknown-linux-gnu" ]]; then
    if command -v x86_64-unknown-linux-gnu-gcc >/dev/null 2>&1; then
      env+=("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=$(command -v x86_64-unknown-linux-gnu-gcc)")
      if command -v x86_64-unknown-linux-gnu-ar >/dev/null 2>&1; then
        env+=("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_AR=$(command -v x86_64-unknown-linux-gnu-ar)")
      fi
    fi
  elif [[ "${target}" == "aarch64-unknown-linux-gnu" ]]; then
    if command -v aarch64-unknown-linux-gnu-gcc >/dev/null 2>&1; then
      env+=("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=$(command -v aarch64-unknown-linux-gnu-gcc)")
      if command -v aarch64-unknown-linux-gnu-ar >/dev/null 2>&1; then
        env+=("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_AR=$(command -v aarch64-unknown-linux-gnu-ar)")
      fi
    fi
  fi

  if [[ "${target}" == "aarch64-unknown-linux-gnu" ]] && (( ${#env[@]} == 0 )); then
    echo "!! Skipping ${label}: missing aarch64-unknown-linux-gnu cross toolchain (install via Homebrew)" >&2
    return
  fi

  if (( ${#env[@]} )); then
    if ! env "${env[@]}" ESNODE_VERSION="${VERSION}" ESNODE_TARGET="${target}" ESNODE_ARCH="${arch}" scripts/dist/build-agent.sh; then
      echo "!! Build failed for ${label} (target=${target}); install required toolchain/Deps" >&2
      exit 1
    fi
  else
    if ! ESNODE_VERSION="${VERSION}" ESNODE_TARGET="${target}" ESNODE_ARCH="${arch}" scripts/dist/build-agent.sh; then
      echo "!! Build failed for ${label} (target=${target}); install required toolchain/Deps" >&2
      exit 1
    fi
  fi
  # Copy canonical artifacts into a flattened releases folder for easy pickup.
  local src_base="${ROOT_DIR}/public/distribution/esnode-core"
  local dest="${DEST_ROOT}/${label}"
  mkdir -p "${dest}"
  local rpm_arch="${arch}"
  if [[ "${arch}" == "amd64" ]]; then
    rpm_arch="x86_64"
  elif [[ "${arch}" == "arm64" ]]; then
    rpm_arch="aarch64"
  fi
  for f in \
    "${src_base}/linux/esnode-core-${VERSION}-linux-${arch}.tar.gz" \
    "${src_base}/linux/deb/ubuntu/esnode-core_${VERSION}_${arch}.deb" \
    "${src_base}/linux/rpm/rhel/esnode-core-${VERSION}-1.${rpm_arch}.rpm" \
    "${src_base}/windows/esnode-core-${VERSION}-windows-amd64.zip"
  do
    if [[ -f "${f}" ]]; then
      cp -f "${f}" "${dest}/"
    fi
  done
}

# Always do a host build first (enables Windows zip if target is installed).
call_build "" "${ESNODE_ARCH:-$(uname -m)}" "host"

# Linux amd64 cross-build
call_build "x86_64-unknown-linux-gnu" "amd64" "linux-amd64"

# Linux arm64 cross-build
call_build "aarch64-unknown-linux-gnu" "arm64" "linux-arm64"

echo "==> Release artifacts collected under ${DEST_ROOT}"
find "${DEST_ROOT}" -maxdepth 2 -type f -print
