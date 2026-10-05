#!/bin/sh
# vox installer - https://github.com/rtk-ai/vox
# Usage: curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh
# Custom install dir: curl -fsSL ... | VOX_INSTALL_DIR=~/.local/bin sh
# NVIDIA GPU build:   curl -fsSL ... | VOX_GPU=cuda sh
#
# VOX_GPU picks the build:
#   auto  (default) on Linux x86_64 with an NVIDIA card, ask which build to
#         install when a terminal is reachable; with no terminal, install the
#         CPU build and say how to get the CUDA one.
#   cuda  install the CUDA build (Linux x86_64 only). If it does not start on
#         this machine, or the release has none, the CPU build is installed.
#   cpu   install the CPU build without asking.
# Apple Silicon always gets the Metal build: it is the only macOS build.
#
# The GPU is only used by Whisper (speech-to-text) and by the qwen-native
# backend (voice cloning). The default voices run on the CPU in every build.
#
# For tests/install_test.sh only:
#   VOX_RELEASE_BASE_URL  where release assets are downloaded from
#   VOX_VERSION           release tag to install (skips the GitHub API lookup)
#   VOX_TTY               device the question is asked on (default /dev/tty)
#   VOX_WSL_NVIDIA_SMI    where WSL keeps nvidia-smi (default /usr/lib/wsl/lib/nvidia-smi)

set -e

REPO="rtk-ai/vox"
BINARY_NAME="vox"
DEFAULT_INSTALL_DIR="/usr/local/bin"
FALLBACK_INSTALL_DIR="${HOME}/.local/bin"
INSTALLER_URL="https://raw.githubusercontent.com/${REPO}/main/install.sh"
RELEASE_BASE_URL="${VOX_RELEASE_BASE_URL:-https://github.com/${REPO}/releases/download}"
VERSION="${VOX_VERSION:-}"
GPU="${VOX_GPU:-auto}"
# Answers are read from the terminal, never from stdin: under `curl ... | sh`
# stdin is this script.
TTY_DEVICE="${VOX_TTY:-/dev/tty}"
# WSL ships nvidia-smi here and adds the directory to PATH only for login
# shells: an installer run over ssh or from an agent would not see the card.
WSL_NVIDIA_SMI="${VOX_WSL_NVIDIA_SMI:-/usr/lib/wsl/lib/nvidia-smi}"

if [ -n "${VOX_INSTALL_DIR:-}" ]; then
    INSTALL_DIR="$VOX_INSTALL_DIR"
    EXPLICIT_INSTALL_DIR=1
else
    INSTALL_DIR="$DEFAULT_INSTALL_DIR"
    EXPLICIT_INSTALL_DIR=""
fi

TEMP_DIR=""
STAGE_DIR=""
HAS_NVIDIA=""
# The build asked for, then the build actually installed: cpu, cuda or metal.
BUILD=""
# 1 once the installed binary has been run and started.
STARTS=""
# What a binary that did not start printed on stderr.
START_ERROR=""
# Oldest GPU generation the CUDA build runs on: compute capability 8.x.
MIN_COMPUTE_CAP_MAJOR=8
# The card's compute capability when it is below that, e.g. 6.1.
CARD_TOO_OLD=""
# Why a CUDA build was replaced by the CPU one: no-asset, no-start or fetch.
CUDA_FALLBACK=""

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() {
    printf "${GREEN}[INFO]${NC} %s\n" "$1"
}

warn() {
    printf "${YELLOW}[WARN]${NC} %s\n" "$1" >&2
}

error() {
    printf "${RED}[ERROR]${NC} %s\n" "$1" >&2
    exit 1
}

cleanup() {
    if [ -n "$TEMP_DIR" ]; then
        rm -rf "$TEMP_DIR"
    fi
}

check_gpu_option() {
    case "$GPU" in
        auto|cuda|cpu) ;;
        *) error "Invalid VOX_GPU value '$GPU'. Use one of: auto, cuda, cpu.";;
    esac
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

    if [ "$OS" = "darwin" ] && [ "$ARCH" = "x86_64" ]; then
        # A shell running under Rosetta reports x86_64 on an Apple Silicon Mac.
        if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || true)" = "1" ]; then
            info "x86_64 shell on Apple Silicon (Rosetta): installing the native arm64 build"
            ARCH="aarch64"
        else
            error "Intel Macs are not supported: there is no vox build for x86_64 macOS, because the ONNX runtime used by the piper voices has no prebuilt Intel Mac binary. vox has builds for Apple Silicon Macs, Linux (x86_64, arm64) and Windows (x86_64)."
        fi
    fi
}

get_target() {
    case "$OS" in
        darwin) TARGET="${ARCH}-apple-darwin";;
        linux)
            TARGET="${ARCH}-unknown-linux-gnu"
            ;;
    esac
}

# The Linux binaries need glibc 2.39: they are built on Ubuntu 24.04, and the
# ONNX runtime inside the piper voices uses symbols glibc gained in 2.38, so
# no rebuild on an older system gets around it. Saying so before the download
# beats a loader error after it. A system that does not report a glibc version
# (no getconf, or another C library) is left to the start check.
check_glibc() {
    [ "$OS" = "linux" ] || return 0
    libc=$(getconf GNU_LIBC_VERSION 2>/dev/null || true)
    case "$libc" in
        "glibc "[0-9]*.[0-9]*) ;;
        *) return 0;;
    esac
    libc_version="${libc#glibc }"
    libc_major="${libc_version%%.*}"
    libc_minor="${libc_version#*.}"
    libc_minor="${libc_minor%%.*}"
    case "$libc_major$libc_minor" in
        *[!0-9]*) return 0;;
    esac
    if [ "$libc_major" -lt 2 ] || { [ "$libc_major" -eq 2 ] && [ "$libc_minor" -lt 39 ]; }; then
        error "vox needs glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40 and later); this system has glibc $libc_version. Building from source does not help: the ONNX runtime used by the piper voices needs glibc 2.38 itself."
    fi
}

# An NVIDIA card counts as present only when nvidia-smi runs successfully: the
# tool being installed without a working driver is not enough.
detect_nvidia() {
    HAS_NVIDIA=""
    CARD_TOO_OLD=""
    [ "$OS" = "linux" ] || return 0
    nvidia_smi=""
    if command -v nvidia-smi >/dev/null 2>&1 && nvidia-smi >/dev/null 2>&1; then
        nvidia_smi="nvidia-smi"
    elif [ -x "$WSL_NVIDIA_SMI" ] && "$WSL_NVIDIA_SMI" >/dev/null 2>&1; then
        nvidia_smi="$WSL_NVIDIA_SMI"
    fi
    [ -n "$nvidia_smi" ] || return 0
    HAS_NVIDIA=1

    # The CUDA build is compiled for compute capability 8.0 (RTX 30 series)
    # and newer. On an older card it would start, pass the check below, and
    # then fail on the first GPU operation. An answer that is not a number
    # (an old driver without this query) is taken as unknown, not as too old.
    card_cap=$("$nvidia_smi" --query-gpu=compute_cap --format=csv,noheader 2>/dev/null | sed -n '1{s/ //g;p;}')
    case "$card_cap" in
        [0-9].[0-9]|[0-9][0-9].[0-9])
            if [ "${card_cap%%.*}" -lt "$MIN_COMPUTE_CAP_MAJOR" ]; then
                CARD_TOO_OLD="$card_cap"
            fi
            ;;
    esac
}

# True when somebody can be asked: a terminal is reachable, and this is not a
# CI job, where a pseudo-terminal exists and nobody is there to answer.
can_ask() {
    [ -z "${CI:-}" ] && tty_reachable
}

# True when the terminal can be both read and written. Reading is tried first:
# it never creates anything.
tty_reachable() {
    ( : < "$TTY_DEVICE" ) 2>/dev/null && ( : >> "$TTY_DEVICE" ) 2>/dev/null
}

# The decision behind the question, separate from the terminal so that it can
# be tested: only an explicit yes selects the CUDA build.
answer_is_yes() {
    case "$1" in
        [yY]|[yY][eE][sS]) return 0;;
        *)                 return 1;;
    esac
}

# Asks on the terminal whether to install the CUDA build. Default answer: No.
ask_cuda() {
    # Appending is the same as writing on a terminal, and lets a test stand a
    # plain file in for it without the question erasing the answer.
    {
        printf '\n'
        printf 'An NVIDIA card was detected. vox has two builds for this machine:\n'
        printf '  CPU   works everywhere, nothing else to install.\n'
        printf '  CUDA  faster voice cloning (qwen-native backend) and faster\n'
        printf '        transcription (Whisper). The default voices run on the CPU\n'
        printf '        either way. It needs an RTX 30 series card or newer, the\n'
        printf '        NVIDIA driver and the CUDA 12 runtime libraries cuBLAS and\n'
        printf '        cuRAND. If it does not start, the CPU build is installed.\n'
        printf 'Install the CUDA build? [y/N] '
    } >> "$TTY_DEVICE"
    answer=""
    read -r answer < "$TTY_DEVICE" || true
    answer_is_yes "$answer"
}

# Sets BUILD to the build to ask the release for.
choose_build() {
    if [ "$OS" = "darwin" ]; then
        BUILD="metal"
        case "$GPU" in
            cuda) warn "VOX_GPU=cuda does not apply on macOS: CUDA is for NVIDIA cards. Installing the Metal build, which uses the Apple GPU.";;
            cpu)  warn "VOX_GPU=cpu does not apply on macOS: the only macOS build is the Metal one. Installing it.";;
        esac
        info "Apple Silicon: this build uses Metal (the Apple GPU)"
        return 0
    fi

    BUILD="cpu"

    if [ "$ARCH" != "x86_64" ]; then
        if [ "$GPU" = "cuda" ]; then
            warn "VOX_GPU=cuda does not apply here: the CUDA build exists only for Linux x86_64, and this machine is Linux arm64. Installing the CPU build."
        fi
        return 0
    fi

    if [ -n "$CARD_TOO_OLD" ] && [ "$GPU" != "cpu" ]; then
        warn "This NVIDIA card has compute capability $CARD_TOO_OLD. The CUDA build needs 8.0 or newer (RTX 30 series and later), so the CPU build is installed."
        return 0
    fi

    case "$GPU" in
        cpu) ;;
        cuda)
            BUILD="cuda"
            if [ -z "$HAS_NVIDIA" ]; then
                warn "VOX_GPU=cuda, but no NVIDIA card was found (nvidia-smi is missing or failed). Trying the CUDA build anyway; the CPU build is installed if it does not start."
            fi
            ;;
        auto)
            if [ -n "$HAS_NVIDIA" ]; then
                if can_ask; then
                    if ask_cuda; then
                        BUILD="cuda"
                    fi
                else
                    info "NVIDIA card detected, and nobody to ask (no terminal, or a CI job): installing the CPU build. For the CUDA build, run the installer with VOX_GPU=cuda."
                fi
            fi
            ;;
    esac
}

get_latest_version() {
    if [ -n "$VERSION" ]; then
        return 0
    fi
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$VERSION" ]; then
        error "Failed to get latest version"
    fi
}

# fetch_build SUFFIX
# Downloads vox-${TARGET}${SUFFIX}.tar.gz and unpacks it into STAGE_DIR.
# Returns 0 on success, 1 when the release has no such asset, 2 on any other
# failure (FETCH_ERROR then says what went wrong). Every step is checked by
# hand: callers test the status, and `set -e` is off inside a tested function.
fetch_build() {
    fetch_url="${RELEASE_BASE_URL}/${VERSION}/${BINARY_NAME}-${TARGET}${1}.tar.gz"
    fetch_archive="${TEMP_DIR}/${BINARY_NAME}${1}.tar.gz"
    STAGE_DIR="${TEMP_DIR}/stage${1}"
    FETCH_ERROR=""

    info "Downloading from: $fetch_url"
    fetch_status=0
    fetch_http=$(curl -fsSL -o "$fetch_archive" -w '%{http_code}' "$fetch_url" 2>"${TEMP_DIR}/curl.err") || fetch_status=$?
    if [ "$fetch_status" -ne 0 ]; then
        FETCH_ERROR=$(cat "${TEMP_DIR}/curl.err" 2>/dev/null || true)
        # 404 over HTTP. Exit status 37 is curl's "could not read file", the
        # same thing for the file:// URLs the tests use.
        if [ "$fetch_http" = "404" ] || [ "$fetch_status" -eq 37 ]; then
            return 1
        fi
        return 2
    fi

    info "Extracting..."
    if ! mkdir -p "$STAGE_DIR" || ! tar -xzf "$fetch_archive" -C "$STAGE_DIR"; then
        FETCH_ERROR="could not extract $fetch_archive"
        return 2
    fi
    if [ ! -f "${STAGE_DIR}/${BINARY_NAME}" ]; then
        FETCH_ERROR="the archive does not contain ${BINARY_NAME}"
        return 2
    fi
    # chmod before moving: after `sudo mv` the user may not own the file anymore
    if ! chmod +x "${STAGE_DIR}/${BINARY_NAME}"; then
        FETCH_ERROR="could not make ${BINARY_NAME} executable"
        return 2
    fi
    return 0
}

# Runs the binary with --version. When it fails, START_ERROR holds what it
# said: the reason is not always a missing library (a temp directory mounted
# noexec, a wrong architecture), and guessing would send the user the wrong way.
binary_starts() {
    START_ERROR=""
    if START_ERROR=$("$1" --version 2>&1 >/dev/null); then
        START_ERROR=""
        return 0
    fi
    return 1
}

report_start_error() {
    [ -n "$START_ERROR" ] || return 0
    warn "It said:"
    printf '%s\n' "$START_ERROR" | sed 5q | while IFS= read -r line; do
        warn "  $line"
    done
}

# Lists the shared libraries the binary needs and the system does not have.
report_missing_libraries() {
    if ! command -v ldd >/dev/null 2>&1; then
        warn "ldd is not available, so the missing libraries cannot be listed."
        return 0
    fi
    missing=$(ldd "$1" 2>/dev/null | grep 'not found' || true)
    if [ -z "$missing" ]; then
        warn "ldd reports no missing library."
        return 0
    fi
    warn "Missing libraries:"
    printf '%s\n' "$missing" | while IFS= read -r line; do
        warn "  $(printf '%s' "$line" | sed 's/^[[:space:]]*//')"
    done
}

# True when a CUDA build that starts on this machine is in STAGE_DIR.
# Otherwise says why and sets CUDA_FALLBACK.
try_cuda_build() {
    fetch_result=0
    fetch_build "-cuda" || fetch_result=$?

    case "$fetch_result" in
        0)
            if binary_starts "${STAGE_DIR}/${BINARY_NAME}"; then
                return 0
            fi
            CUDA_FALLBACK="no-start"
            warn "The CUDA build does not start on this machine."
            report_start_error
            report_missing_libraries "${STAGE_DIR}/${BINARY_NAME}"
            warn "It needs the NVIDIA driver (libcuda.so.1) and the CUDA 12 runtime libraries cuBLAS (libcublas.so.12, libcublasLt.so.12) and cuRAND (libcurand.so.10)."
            ;;
        1)
            CUDA_FALLBACK="no-asset"
            warn "Release $VERSION has no CUDA build."
            ;;
        *)
            CUDA_FALLBACK="fetch"
            warn "Could not get the CUDA build: $FETCH_ERROR"
            ;;
    esac
    warn "Installing the CPU build instead."
    return 1
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
        mv "${STAGE_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/"
    elif can_sudo; then
        info "Requesting sudo to install to $INSTALL_DIR"
        sudo mkdir -p "$INSTALL_DIR"
        sudo mv "${STAGE_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/"
    elif [ -z "$EXPLICIT_INSTALL_DIR" ]; then
        warn "$INSTALL_DIR is not writable and sudo is not available."
        warn "Falling back to $FALLBACK_INSTALL_DIR"
        INSTALL_DIR="$FALLBACK_INSTALL_DIR"
        mkdir -p "$INSTALL_DIR"
        mv "${STAGE_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/"
    else
        error "Cannot write to $INSTALL_DIR and sudo is not available. Set VOX_INSTALL_DIR to a writable directory."
    fi
}

install() {
    info "Detected: $OS $ARCH"
    info "Target: $TARGET"
    info "Version: $VERSION"

    TEMP_DIR=$(mktemp -d)

    # The CUDA build is run before it is placed: it links the CUDA libraries
    # dynamically and does not start at all on a machine without them.
    if [ "$BUILD" = "cuda" ] && ! try_cuda_build; then
        BUILD="cpu"
    fi

    if [ "$BUILD" != "cuda" ]; then
        fetch_result=0
        fetch_build "" || fetch_result=$?
        if [ "$fetch_result" -ne 0 ]; then
            error "Failed to get the ${BINARY_NAME} binary for $TARGET from release $VERSION: $FETCH_ERROR"
        fi
    fi

    place_binary

    info "Successfully installed ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}"
}

check_deps() {
    if [ "$OS" = "linux" ]; then
        if ! ldconfig -p 2>/dev/null | grep -q libasound; then
            warn "ALSA not found, and vox needs it to start. Install it: sudo apt install libasound2t64 (libasound2 before Ubuntu 24.04), or sudo dnf install alsa-lib"
        fi
    fi
}

verify() {
    # The file just installed, not whichever vox comes first on PATH.
    if installed_version=$("${INSTALL_DIR}/${BINARY_NAME}" --version 2>/dev/null); then
        info "Verification: $installed_version"
        STARTS=1
    else
        warn "${INSTALL_DIR}/${BINARY_NAME} is installed but does not start."
        binary_starts "${INSTALL_DIR}/${BINARY_NAME}" || report_start_error
        report_missing_libraries "${INSTALL_DIR}/${BINARY_NAME}"
        case "$START_ERROR" in
            *GLIBC_*)
                warn "This system's C library (glibc) is older than vox needs: glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40 and later)."
                ;;
        esac
    fi

    if ! command -v "$BINARY_NAME" >/dev/null 2>&1; then
        warn "Binary installed but not in PATH. Add $INSTALL_DIR to your PATH."
    fi
}

# The command that installs the CUDA build over this one.
print_cuda_command() {
    if [ -n "$EXPLICIT_INSTALL_DIR" ]; then
        info "  curl -fsSL $INSTALLER_URL | VOX_GPU=cuda VOX_INSTALL_DIR=\"$INSTALL_DIR\" sh"
    else
        info "  curl -fsSL $INSTALLER_URL | VOX_GPU=cuda sh"
    fi
}

# Last lines: which build was installed and, where there is one, how to switch.
report_build() {
    case "$BUILD" in
        metal)
            info "Build installed: Metal (Apple GPU). The GPU is used by Whisper transcription and qwen-native voice cloning; the default voices run on the CPU."
            ;;
        cuda)
            info "Build installed: CUDA (NVIDIA GPU). The GPU is used by Whisper transcription and qwen-native voice cloning; the default voices run on the CPU."
            ;;
        *)
            info "Build installed: CPU."
            if [ "$ARCH" != "x86_64" ]; then
                if [ -n "$HAS_NVIDIA" ]; then
                    info "This machine has an NVIDIA card, but the CUDA build exists only for Linux x86_64."
                fi
                return 0
            fi
            case "$CUDA_FALLBACK" in
                no-start)
                    info "The CUDA build was left out because it does not start here. Once the NVIDIA driver and the CUDA 12 runtime libraries are installed, switch to it with:"
                    print_cuda_command
                    ;;
                no-asset)
                    info "The CUDA build was left out because release $VERSION does not have one. On a release that has it, switch with:"
                    print_cuda_command
                    ;;
                fetch)
                    info "The CUDA build was left out because it could not be downloaded or unpacked. To try again:"
                    print_cuda_command
                    ;;
                *)
                    if [ -n "$CARD_TOO_OLD" ]; then
                        info "This machine's NVIDIA card (compute capability $CARD_TOO_OLD) is older than the CUDA build supports (8.0, RTX 30 series)."
                    elif [ -n "$HAS_NVIDIA" ]; then
                        info "This machine has an NVIDIA card. The CUDA build makes qwen-native voice cloning and Whisper transcription faster, and needs the NVIDIA driver and the CUDA 12 runtime libraries. To switch to it:"
                        print_cuda_command
                    fi
                    ;;
            esac
            ;;
    esac
}

main() {
    info "Installing $BINARY_NAME..."

    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    check_gpu_option
    detect_platform
    detect_arch
    check_glibc
    get_target
    detect_nvidia
    choose_build
    get_latest_version
    install
    check_deps
    verify

    echo ""
    if [ -z "$STARTS" ]; then
        # Not a success to report: the file is in place and cannot run.
        error "${BINARY_NAME} was copied to ${INSTALL_DIR}/${BINARY_NAME} but does not start on this machine. See the messages above."
    fi
    info "Installation complete! Run '$BINARY_NAME --help' to get started."
    report_build
}

main
