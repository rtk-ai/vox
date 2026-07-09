#!/bin/sh
# vox installer - https://github.com/rtk-ai/vox
# Usage: curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh
# Custom install dir: curl -fsSL ... | VOX_INSTALL_DIR=~/.local/bin sh

set -e

REPO="rtk-ai/vox"
BINARY_NAME="vox"
DEFAULT_INSTALL_DIR="/usr/local/bin"
FALLBACK_INSTALL_DIR="${HOME}/.local/bin"

if [ -n "${VOX_INSTALL_DIR:-}" ]; then
    INSTALL_DIR="$VOX_INSTALL_DIR"
    EXPLICIT_INSTALL_DIR=1
else
    INSTALL_DIR="$DEFAULT_INSTALL_DIR"
    EXPLICIT_INSTALL_DIR=""
fi

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() {
    printf "${GREEN}[INFO]${NC} %s\n" "$1"
}

warn() {
    printf "${YELLOW}[WARN]${NC} %s\n" "$1"
}

error() {
    printf "${RED}[ERROR]${NC} %s\n" "$1"
    exit 1
}

detect_platform() {
    case "$(uname -s)" in
        Darwin*) OS="darwin";;
        Linux*)  OS="linux";;
        *)       error "Unsupported OS: $(uname -s). Use WSL on Windows.";;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  ARCH="x86_64";;
        arm64|aarch64) ARCH="aarch64";;
        *)             error "Unsupported architecture: $(uname -m)";;
    esac
}

get_target() {
    case "$OS" in
        darwin) TARGET="${ARCH}-apple-darwin";;
        linux)
            TARGET="${ARCH}-unknown-linux-gnu"
            ;;
    esac
}

get_latest_version() {
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$VERSION" ]; then
        error "Failed to get latest version"
    fi
}

# True when sudo can actually be used: either cached/passwordless credentials,
# or a controlling terminal is available for the password prompt (a plain
# `curl | sh` has no usable stdin, but sudo prompts via /dev/tty).
can_sudo() {
    command -v sudo >/dev/null 2>&1 || return 1
    if sudo -n true 2>/dev/null; then
        return 0
    fi
    ( : < /dev/tty ) 2>/dev/null
}

place_binary() {
    mkdir -p "$INSTALL_DIR" 2>/dev/null || true

    if [ -d "$INSTALL_DIR" ] && [ -w "$INSTALL_DIR" ]; then
        mv "${TEMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/"
    elif can_sudo; then
        info "Requesting sudo to install to $INSTALL_DIR"
        sudo mkdir -p "$INSTALL_DIR"
        sudo mv "${TEMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/"
    elif [ -z "$EXPLICIT_INSTALL_DIR" ]; then
        warn "$INSTALL_DIR is not writable and sudo is not available."
        warn "Falling back to $FALLBACK_INSTALL_DIR"
        INSTALL_DIR="$FALLBACK_INSTALL_DIR"
        mkdir -p "$INSTALL_DIR"
        mv "${TEMP_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/"
    else
        error "Cannot write to $INSTALL_DIR and sudo is not available. Set VOX_INSTALL_DIR to a writable directory."
    fi
}

install() {
    info "Detected: $OS $ARCH"
    info "Target: $TARGET"
    info "Version: $VERSION"

    DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${BINARY_NAME}-${TARGET}.tar.gz"
    TEMP_DIR=$(mktemp -d)
    ARCHIVE="${TEMP_DIR}/${BINARY_NAME}.tar.gz"

    info "Downloading from: $DOWNLOAD_URL"
    if ! curl -fsSL "$DOWNLOAD_URL" -o "$ARCHIVE"; then
        error "Failed to download binary"
    fi

    info "Extracting..."
    tar -xzf "$ARCHIVE" -C "$TEMP_DIR"

    # chmod before moving: after `sudo mv` the user may not own the file anymore
    chmod +x "${TEMP_DIR}/${BINARY_NAME}"

    place_binary

    rm -rf "$TEMP_DIR"

    info "Successfully installed ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}"
}

check_deps() {
    if [ "$OS" = "linux" ]; then
        if ! ldconfig -p 2>/dev/null | grep -q libasound; then
            warn "ALSA not found. Install it: sudo apt install libasound2-dev"
        fi
    fi
}

verify() {
    if command -v "$BINARY_NAME" >/dev/null 2>&1; then
        info "Verification: $($BINARY_NAME --version)"
    else
        warn "Binary installed but not in PATH. Add $INSTALL_DIR to your PATH."
    fi
}

main() {
    info "Installing $BINARY_NAME..."

    detect_platform
    detect_arch
    get_target
    get_latest_version
    install
    check_deps
    verify

    echo ""
    info "Installation complete! Run '$BINARY_NAME --help' to get started."
}

main
