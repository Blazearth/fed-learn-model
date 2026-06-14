#!/usr/bin/env bash
# install.sh — Install the Federated Learning Client Daemon
#
# Usage:
#   sudo ./install.sh
#
# Requirements:
#   - Linux (systemd-based distribution)
#   - Root privileges for installation
#   - Binary must be built first: cargo build --release

set -euo pipefail

# ── Configuration ─────────────────────────────────────────────────────────────

BINARY_NAME="fl-client-daemon"
BINARY_SRC="target/release/${BINARY_NAME}"
BINARY_DEST="/usr/local/bin/${BINARY_NAME}"

SERVICE_NAME="rust-client-daemon"
SERVICE_SRC="config/${SERVICE_NAME}.service"
SERVICE_DEST="/etc/systemd/system/${SERVICE_NAME}.service"

CONFIG_DIR="/etc/fl-daemon"
CONFIG_SRC="config/config.example.toml"
CONFIG_DEST="${CONFIG_DIR}/config.toml"

WORK_DIR="/var/lib/fl-daemon"
LOG_DIR="/var/log/fl-daemon"
CERT_DIR="${CONFIG_DIR}/certs"

DAEMON_USER="fl-daemon"
DAEMON_GROUP="fl-daemon"

# ── Colour helpers ─────────────────────────────────────────────────────────────

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

# ── Precondition checks ───────────────────────────────────────────────────────

if [[ $EUID -ne 0 ]]; then
    error "This script must be run as root (sudo ./install.sh)"
    exit 1
fi

if [[ ! -f "${BINARY_SRC}" ]]; then
    error "Binary not found at ${BINARY_SRC}"
    error "Build first with: cargo build --release"
    exit 1
fi

if [[ ! -f "${SERVICE_SRC}" ]]; then
    error "Service file not found at ${SERVICE_SRC}"
    exit 1
fi

# ── Create daemon user and group ──────────────────────────────────────────────

info "Creating daemon user and group..."
if ! getent group "${DAEMON_GROUP}" >/dev/null 2>&1; then
    groupadd --system "${DAEMON_GROUP}"
    info "Created group: ${DAEMON_GROUP}"
else
    info "Group already exists: ${DAEMON_GROUP}"
fi

if ! getent passwd "${DAEMON_USER}" >/dev/null 2>&1; then
    useradd \
        --system \
        --gid "${DAEMON_GROUP}" \
        --home-dir "${WORK_DIR}" \
        --no-create-home \
        --shell /sbin/nologin \
        --comment "Federated Learning Client Daemon" \
        "${DAEMON_USER}"
    info "Created user: ${DAEMON_USER}"
else
    info "User already exists: ${DAEMON_USER}"
fi

# Add user to tss group for TPM access (if group exists)
if getent group tss >/dev/null 2>&1; then
    usermod -aG tss "${DAEMON_USER}" && info "Added ${DAEMON_USER} to tss group for TPM access"
fi

# ── Create directory structure ────────────────────────────────────────────────

info "Creating directory structure..."

# Configuration directory
install -d -m 750 -o root -g "${DAEMON_GROUP}" "${CONFIG_DIR}"
install -d -m 750 -o root -g "${DAEMON_GROUP}" "${CERT_DIR}"

# Working directory (models, checkpoints, audit log)
install -d -m 750 -o "${DAEMON_USER}" -g "${DAEMON_GROUP}" "${WORK_DIR}"
install -d -m 750 -o "${DAEMON_USER}" -g "${DAEMON_GROUP}" "${WORK_DIR}/models"
install -d -m 750 -o "${DAEMON_USER}" -g "${DAEMON_GROUP}" "${WORK_DIR}/checkpoints"
install -d -m 750 -o "${DAEMON_USER}" -g "${DAEMON_GROUP}" "${WORK_DIR}/models/archive"

# Log directory
install -d -m 750 -o "${DAEMON_USER}" -g "${DAEMON_GROUP}" "${LOG_DIR}"

info "Directories created:"
info "  Config:       ${CONFIG_DIR}"
info "  Certificates: ${CERT_DIR}"
info "  Working dir:  ${WORK_DIR}"
info "  Log dir:      ${LOG_DIR}"

# ── Install binary ────────────────────────────────────────────────────────────

info "Installing binary..."
install -m 755 -o root -g root "${BINARY_SRC}" "${BINARY_DEST}"
info "Binary installed to: ${BINARY_DEST}"

# ── Install example configuration ────────────────────────────────────────────

if [[ -f "${CONFIG_DEST}" ]]; then
    warn "Configuration file already exists at ${CONFIG_DEST}"
    warn "Skipping to avoid overwriting existing configuration"
    warn "See ${CONFIG_SRC} for reference"
else
    if [[ -f "${CONFIG_SRC}" ]]; then
        install -m 640 -o root -g "${DAEMON_GROUP}" "${CONFIG_SRC}" "${CONFIG_DEST}"
        info "Example configuration installed to: ${CONFIG_DEST}"
        warn "IMPORTANT: Edit ${CONFIG_DEST} before starting the service"
    else
        warn "Example config not found at ${CONFIG_SRC} — skipping"
        warn "Create ${CONFIG_DEST} manually before starting the service"
    fi
fi

# ── Install example schema files ─────────────────────────────────────────────

for schema_file in config/*.schema.json; do
    if [[ -f "${schema_file}" ]]; then
        dest="${CONFIG_DIR}/$(basename "${schema_file}")"
        install -m 640 -o root -g "${DAEMON_GROUP}" "${schema_file}" "${dest}"
        info "Schema installed: ${dest}"
    fi
done

# ── Install systemd service ───────────────────────────────────────────────────

info "Installing systemd service..."
install -m 644 -o root -g root "${SERVICE_SRC}" "${SERVICE_DEST}"
systemctl daemon-reload
info "Service installed: ${SERVICE_DEST}"

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
info "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
info "Installation complete!"
info ""
info "Next steps:"
info "  1. Configure TPM/HSM for private key storage (see docs/SECURITY.md)"
info "  2. Install organization certificate to ${CERT_DIR}/"
info "  3. Edit configuration: ${CONFIG_DEST}"
info "  4. Enable and start the service:"
info "       sudo systemctl enable ${SERVICE_NAME}"
info "       sudo systemctl start ${SERVICE_NAME}"
info "  5. Monitor logs:"
info "       sudo journalctl -u ${SERVICE_NAME} -f"
info "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
