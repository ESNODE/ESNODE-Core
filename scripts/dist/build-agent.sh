#!/usr/bin/env bash
set -euo pipefail

# Build and package ESNODE-Core for distribution.
# Produces (best-effort):
# - public/distribution/esnode-core/linux/esnode-core-<version>-linux-amd64.tar.gz
# - public/distribution/esnode-core/windows/esnode-core-<version>-windows-amd64.zip (if target installed)
# - public/distribution/esnode-core/linux/deb|rpm (if fpm is installed)
#
# Usage:
#   ESNODE_VERSION=1.0.0 scripts/dist/build-agent.sh
#   scripts/dist/build-agent.sh 1.0.0

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${ESNODE_VERSION:-${1:-}}"
if [[ -z "${VERSION}" ]]; then
  echo "ERROR: provide version via ESNODE_VERSION env or first arg" >&2
  exit 1
fi

TARGET="${ESNODE_TARGET:-}"
if [[ -n "${TARGET}" ]]; then
  echo "==> Building esnode-core (version ${VERSION}) for target ${TARGET}"
else
  echo "==> Building esnode-core (version ${VERSION}) for host"
fi
cd "${ROOT_DIR}"
if [[ -n "${TARGET}" ]]; then
  cargo build --release --target "${TARGET}" --bin esnode-core
else
  cargo build --release --bin esnode-core
fi

OUT_DIR_PUBLIC="${ROOT_DIR}/public/distribution/esnode-core"
BIN="${ROOT_DIR}/target/${TARGET:+${TARGET}/}release/esnode-core"
SERVICE_FILE="${ROOT_DIR}/deploy/systemd/esnode-core.service"
INSTALL_SH="${ROOT_DIR}/scripts/install/esnode-core-linux.sh"
if [[ ! -f "${BIN}" ]]; then
  echo "ERROR: built binary not found at ${BIN}" >&2
  exit 1
fi

host_arch="$(uname -m)"
deb_arch="${ESNODE_ARCH:-${host_arch}}"
case "${deb_arch}" in
  x86_64|amd64) deb_arch="amd64"; rpm_arch="x86_64" ;;
  aarch64|arm64) deb_arch="arm64"; rpm_arch="aarch64" ;;
  *)
    echo "ERROR: unsupported architecture ${deb_arch}. Override with ESNODE_ARCH=amd64|arm64." >&2
    exit 1
    ;;
esac

tar_bin() { command -v gtar >/dev/null 2>&1 && echo "$(command -v gtar)" || echo "$(command -v tar)"; }
have_fpm() { command -v fpm >/dev/null 2>&1; }

mkdir -p "${OUT_DIR_PUBLIC}/linux"
TAR_NAME="esnode-core-${VERSION}-linux-${deb_arch}.tar.gz"
echo "==> Creating generic tarball: ${TAR_NAME}"
tmp_tar_dir="$(mktemp -d)"
cp "${BIN}" "${tmp_tar_dir}/esnode-core"
if [[ -f "${SERVICE_FILE}" ]]; then
  cp "${SERVICE_FILE}" "${tmp_tar_dir}/esnode-core.service"
fi
if [[ -f "${INSTALL_SH}" ]]; then
  cp "${INSTALL_SH}" "${tmp_tar_dir}/install-esnode-core.sh"
fi
COPYFILE_DISABLE=1 "$(tar_bin)" --format=gnu -C "${tmp_tar_dir}" -czf "${OUT_DIR_PUBLIC}/linux/${TAR_NAME}" .
rm -rf "${tmp_tar_dir}"

pkg_with_fpm() {
  local pkg_type=$1
  local output=$2
  local iter=${3:-1}
  local tmpdir
  tmpdir="$(mktemp -d)"
  mkdir -p "${tmpdir}/usr/local/bin" "${tmpdir}/etc/systemd/system" "${tmpdir}/var/lib/esnode/tsdb"
  cp "${BIN}" "${tmpdir}/usr/local/bin/esnode-core"
  if [[ -f "${SERVICE_FILE}" ]]; then
    cp "${SERVICE_FILE}" "${tmpdir}/etc/systemd/system/esnode-core.service"
  fi
  cat >"${tmpdir}/postinstall.sh" <<'EOF'
#!/bin/sh
set -e
mkdir -p /var/lib/esnode/tsdb
chmod 755 /var/lib/esnode /var/lib/esnode/tsdb
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload || true
  systemctl enable --now esnode-core.service || true
fi
EOF
  chmod +x "${tmpdir}/postinstall.sh"
  rm -f "${output}"
  COPYFILE_DISABLE=1 TAR="$(tar_bin)" TAR_OPTIONS="--format=gnu --owner=0 --group=0" fpm -s dir -t "${pkg_type}" \
    -n esnode-core \
    -v "${VERSION}" \
    --iteration "${iter}" \
    --architecture "$([[ "${pkg_type}" == "deb" ]] && echo "${deb_arch}" || echo "${rpm_arch}")" \
    --description "ESNODE-Core: GPU-aware node metrics exporter" \
    --license "ESNODE BUSL-style (see LICENSE)" \
    --url "https://esnode.co" \
    --maintainer "Estimatedstocks AB" \
    --package "${output}" \
    --after-install "${tmpdir}/postinstall.sh" \
    --config-files /etc/systemd/system/esnode-core.service \
    -C "${tmpdir}" \
    usr/local/bin/esnode-core \
    etc/systemd/system/esnode-core.service \
    var/lib/esnode/tsdb
  rm -rf "${tmpdir}"
}

if have_fpm; then
  echo "==> fpm detected; building .deb and .rpm"
  mkdir -p \
    "${OUT_DIR_PUBLIC}/linux/deb/ubuntu" \
    "${OUT_DIR_PUBLIC}/linux/deb/debian" \
    "${OUT_DIR_PUBLIC}/linux/deb/dgx" \
    "${OUT_DIR_PUBLIC}/linux/rpm/rhel" \
    "${OUT_DIR_PUBLIC}/linux/rpm/sles"

  pkg_with_fpm "deb" "${OUT_DIR_PUBLIC}/linux/deb/ubuntu/esnode-core_${VERSION}_${deb_arch}.deb"
  cp "${OUT_DIR_PUBLIC}/linux/deb/ubuntu/esnode-core_${VERSION}_${deb_arch}.deb" \
    "${OUT_DIR_PUBLIC}/linux/deb/debian/esnode-core_${VERSION}_${deb_arch}.deb"
  cp "${OUT_DIR_PUBLIC}/linux/deb/ubuntu/esnode-core_${VERSION}_${deb_arch}.deb" \
    "${OUT_DIR_PUBLIC}/linux/deb/dgx/esnode-core_${VERSION}_${deb_arch}.deb"

  pkg_with_fpm "rpm" "${OUT_DIR_PUBLIC}/linux/rpm/rhel/esnode-core-${VERSION}-1.${rpm_arch}.rpm"
  cp "${OUT_DIR_PUBLIC}/linux/rpm/rhel/esnode-core-${VERSION}-1.${rpm_arch}.rpm" \
    "${OUT_DIR_PUBLIC}/linux/rpm/sles/esnode-core-${VERSION}-1.${rpm_arch}.rpm"
else
  echo "!! fpm not found; skipped .deb/.rpm packaging. Install fpm to build native packages." >&2
fi

build_windows() {
  local target="x86_64-pc-windows-gnu"
  if rustup target list | grep -q "^${target} (installed)"; then
    echo "==> Building Windows target (${target})"
    cargo build --release --bin esnode-core --target "${target}"
    local bin_win="${ROOT_DIR}/target/${target}/release/esnode-core.exe"
    if [[ -f "${bin_win}" ]]; then
      mkdir -p "${OUT_DIR_PUBLIC}/windows"
      local zip_name="esnode-core-${VERSION}-windows-amd64.zip"
      local tmpzip
      tmpzip="$(mktemp -d)"
      cp "${bin_win}" "${tmpzip}/esnode-core.exe"
      (cd "${tmpzip}" && zip -q "${OUT_DIR_PUBLIC}/windows/${zip_name}" "esnode-core.exe")
      rm -rf "${tmpzip}"
    else
      echo "!! Windows binary not found at ${bin_win}; skipping zip." >&2
    fi
  else
    echo "!! Windows target ${target} not installed; skipping Windows packaging." >&2
  fi
}

build_windows

echo "Done. Artifacts in ${OUT_DIR_PUBLIC}"
