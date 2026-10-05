#!/bin/sh
# Tests for install.sh. Run from anywhere: sh tests/install_test.sh
#
# Nothing here touches the network or the real system. Each case runs the real
# install.sh with:
#   - PATH reduced to one directory holding links to the few tools the
#     installer needs, plus fakes for `uname`, `nvidia-smi`, `ldd`, `ldconfig`
#     and `sysctl` that simulate the platform, and a `curl` wrapper that
#     records every URL asked for before handing over to the real curl;
#   - VOX_RELEASE_BASE_URL pointing at a fake release on disk (file:// URL)
#     whose tarballs hold a shell script named vox that prints "vox 9.9.9";
#     each one carries a "# build:" marker saying which asset it came from;
#   - VOX_INSTALL_DIR, HOME and TMPDIR inside a temporary directory;
#   - VOX_TTY standing in for /dev/tty: a file holding the answer, or a path
#     that cannot be opened when the case has no terminal.
#
# INSTALL_TEST_SHELL chooses the shell that runs install.sh (default /bin/sh),
# e.g. INSTALL_TEST_SHELL=/bin/dash sh tests/install_test.sh
# INSTALL_TEST_VERBOSE=1 also prints what the installer said in passing cases.
#
# What is NOT covered:
#   - A real terminal, unless python3 is there: the last two cases then run the
#     installer under a pseudo-terminal and answer the question on the real
#     /dev/tty. Without python3 they are skipped, and the question is only
#     exercised through the VOX_TTY file.
#   - The default install directory (/usr/local/bin), sudo, and the fallback to
#     ~/.local/bin: they would write to the real system.
#   - Real release assets, the real GitHub API and real HTTP redirects. The
#     HTTP 404 case uses a local python3 server and is skipped without python3.
#   - A real CUDA binary and the real ldd: "does not start" is a fake vox that
#     exits 127, and the list of missing libraries comes from a fake ldd.
#   - Rosetta on a real Mac: the case uses a fake sysctl.

set -u

ROOT=$(cd "$(dirname "$0")/.." && pwd)
INSTALL_SH="$ROOT/install.sh"
TEST_SHELL="${INSTALL_TEST_SHELL:-/bin/sh}"
TAG="v9.9.9"

WORK=$(mktemp -d "${TMPDIR:-/tmp}/vox-install-test.XXXXXX")
SERVER_PID=""

stop_server() {
    if [ -n "$SERVER_PID" ]; then
        kill "$SERVER_PID" 2>/dev/null
        wait "$SERVER_PID" 2>/dev/null
        SERVER_PID=""
    fi
}

finish() {
    stop_server
    rm -rf "$WORK"
}
trap finish EXIT

REAL_CURL=$(command -v curl) || { echo "curl is required"; exit 2; }
REAL_MKTEMP=$(command -v mktemp) || { echo "mktemp is required"; exit 2; }
PYTHON=$(command -v python3 || true)

PASSED=0
FAILED=0
SKIPPED=0

# ---------------------------------------------------------------- fake release

# make_asset RELEASE ASSET MARKER KIND
# KIND: ok (prints the version), broken (exits 127 like a binary whose shared
# libraries are missing) or corrupt (the tarball is not a tarball).
make_asset() {
    asset_dir="$WORK/release-$1/$TAG"
    mkdir -p "$asset_dir" "$WORK/build"
    case "$4" in
        ok)
            cat > "$WORK/build/vox" <<EOF
#!/bin/sh
# build: $3
echo "vox 9.9.9"
EOF
            ;;
        broken)
            cat > "$WORK/build/vox" <<EOF
#!/bin/sh
# build: $3
echo "vox: error while loading shared libraries: libcublas.so.12: cannot open shared object file" >&2
exit 127
EOF
            ;;
        oldglibc)
            cat > "$WORK/build/vox" <<EOF
#!/bin/sh
# build: $3
echo "vox: /lib/x86_64-linux-gnu/libc.so.6: version GLIBC_2.39 not found (required by vox)" >&2
exit 1
EOF
            ;;
        corrupt)
            echo "this is not a gzip archive" > "$asset_dir/$2.tar.gz"
            return 0
            ;;
    esac
    chmod +x "$WORK/build/vox"
    tar -czf "$asset_dir/$2.tar.gz" -C "$WORK/build" vox
}

MAC_ARM="vox-aarch64-apple-darwin"
MAC_INTEL="vox-x86_64-apple-darwin"
LINUX_X86="vox-x86_64-unknown-linux-gnu"
LINUX_ARM="vox-aarch64-unknown-linux-gnu"
LINUX_CUDA="vox-x86_64-unknown-linux-gnu-cuda"

# "full" even holds an Intel Mac asset and an arm64 CUDA asset, neither of
# which exists for real: a case that must not install them cannot pass just
# because the download failed.
make_asset full "$MAC_ARM" metal ok
make_asset full "$MAC_INTEL" intel-mac ok
make_asset full "$LINUX_X86" cpu-x86_64 ok
make_asset full "$LINUX_ARM" cpu-arm64 ok
make_asset full "$LINUX_CUDA" cuda ok
make_asset full "vox-aarch64-unknown-linux-gnu-cuda" cuda-arm64 ok

make_asset nocuda "$LINUX_X86" cpu-x86_64 ok

make_asset brokencuda "$LINUX_X86" cpu-x86_64 ok
make_asset brokencuda "$LINUX_CUDA" cuda broken

make_asset corruptcuda "$LINUX_X86" cpu-x86_64 ok
make_asset corruptcuda "$LINUX_CUDA" cuda corrupt

make_asset brokencpu "$LINUX_X86" cpu-x86_64 broken

make_asset oldglibc "$LINUX_X86" cpu-x86_64 oldglibc

mkdir -p "$WORK/release-empty/$TAG"

# ------------------------------------------------------------------- the cases

# begin NAME: starts a case with the defaults below. A case then changes what
# it needs, calls run_installer, checks the result and calls end.
begin() {
    CASE_NAME="$1"
    CASE_ERRORS=0
    CASE_DIR=$(mktemp -d "$WORK/case.XXXXXX")
    OUT="$CASE_DIR/out"
    CURL_LOG="$CASE_DIR/curl.log"
    : > "$CURL_LOG"

    SIM_KERNEL="Linux"       # uname -s
    SIM_MACHINE="x86_64"     # uname -m
    SIM_NVIDIA="none"        # none | ok | failing | old (card below 8.0) | noquery
    SIM_LDD="none"           # none | missing (lists two libraries as not found)
    SIM_ALSA="present"       # present | absent | unknown (no ldconfig at all)
    ON_PATH=1                # 0: the install directory is not on PATH
    SIM_ROSETTA=""           # 1: sysctl says the hardware is Apple Silicon
    RELEASE="full"
    BASE_URL=""              # default: file:// URL of $RELEASE
    GPU="-"                  # "-" leaves VOX_GPU unset
    SET_VERSION=1            # 0 leaves VOX_VERSION unset (GitHub API lookup)
    TERMINAL="none"          # none | file (VOX_TTY holds TTY_ANSWER) | pty
    TTY_ANSWER=""
    TTY_EOF=""               # 1: the terminal gives end of file, not even a newline
    RUN_MODE="file"          # file: sh install.sh | pipe: cat install.sh | sh
    SIM_WSL_NVIDIA=""        # 1: nvidia-smi exists only where WSL keeps it, off PATH
    SIM_CI=""                # 1: CI is set, as in a CI job
    SIM_GLIBC=""             # what getconf GNU_LIBC_VERSION prints; empty: no getconf
}

fail() {
    CASE_ERRORS=$((CASE_ERRORS + 1))
    echo "    $1"
}

end() {
    if [ "$CASE_ERRORS" -eq 0 ]; then
        PASSED=$((PASSED + 1))
        echo "ok   - $CASE_NAME"
        if [ -n "${INSTALL_TEST_VERBOSE:-}" ]; then
            sed 's/^/    | /' "$OUT"
        fi
    else
        FAILED=$((FAILED + 1))
        echo "FAIL - $CASE_NAME"
        echo "    ---- installer output (exit status $STATUS)"
        sed 's/^/    | /' "$OUT"
        echo "    ---- URLs asked for"
        sed 's/^/    | /' "$CURL_LOG"
    fi
}

skip() {
    SKIPPED=$((SKIPPED + 1))
    echo "skip - $1"
}

make_bin() {
    bin="$CASE_DIR/bin"
    mkdir -p "$bin"
    # The installer only ever calls `mktemp -d`. macOS ignores TMPDIR for that
    # form, so the directory is made under the case's own tmp, where
    # run_installer can check that nothing is left behind.
    cat > "$bin/mktemp" <<EOF
#!/bin/sh
[ "\$*" = "-d" ] || { echo "fake mktemp: only -d is supported" >&2; exit 2; }
exec "$REAL_MKTEMP" -d "$CASE_DIR/tmp/install.XXXXXX"
EOF

    cat > "$bin/uname" <<EOF
#!/bin/sh
case "\$1" in
    -m) echo "$SIM_MACHINE";;
    *)  echo "$SIM_KERNEL";;
esac
EOF

    # Records every URL, answers the GitHub API itself, and passes the rest on.
    cat > "$bin/curl" <<EOF
#!/bin/sh
for arg in "\$@"; do
    case "\$arg" in
        file://*|http://*|https://*) printf '%s\n' "\$arg" >> "$CURL_LOG";;
    esac
done
for arg in "\$@"; do
    case "\$arg" in
        https://api.github.com/*)
            printf '{\n  "url": "https://api.github.com/repos/rtk-ai/vox/releases/1",\n  "tag_name": "$TAG",\n  "name": "$TAG"\n}\n'
            exit 0
            ;;
    esac
done
exec "$REAL_CURL" "\$@"
EOF

    if [ -n "$SIM_GLIBC" ]; then
        printf '#!/bin/sh\necho "%s"\n' "$SIM_GLIBC" > "$bin/getconf"
        chmod +x "$bin/getconf"
    fi

    case "$SIM_NVIDIA" in
        ok)
            printf '#!/bin/sh\ncase "$*" in *compute_cap*) echo "8.9";; *) echo "NVIDIA-SMI 550.54 Driver Version: 550.54 CUDA Version: 12.4";; esac\n' > "$bin/nvidia-smi"
            ;;
        old)
            # A GTX 10 series card: the driver works, the card predates the build.
            printf '#!/bin/sh\ncase "$*" in *compute_cap*) echo "6.1";; *) echo "NVIDIA-SMI 550.54 Driver Version: 550.54 CUDA Version: 12.4";; esac\n' > "$bin/nvidia-smi"
            ;;
        noquery)
            # An old driver: nvidia-smi runs but does not know the query.
            printf '#!/bin/sh\ncase "$*" in *compute_cap*) echo "Field \"compute_cap\" is not a valid field to query."; exit 2;; *) echo "NVIDIA-SMI 470.57";; esac\n' > "$bin/nvidia-smi"
            ;;
        failing)
            printf '#!/bin/sh\necho "NVIDIA-SMI has failed because it could not communicate with the NVIDIA driver."\nexit 9\n' > "$bin/nvidia-smi"
            ;;
    esac

    if [ "$SIM_LDD" = "missing" ]; then
        cat > "$bin/ldd" <<'EOF'
#!/bin/sh
printf '\tlinux-vdso.so.1 (0x00007ffd1a5f2000)\n'
printf '\tlibcublas.so.12 => not found\n'
printf '\tlibcuda.so.1 => not found\n'
printf '\tlibc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x00007f1c2a000000)\n'
EOF
    fi

    case "$SIM_ALSA" in
        present)
            printf '#!/bin/sh\nprintf "\\tlibasound.so.2 (libc6,x86-64) => /lib/x86_64-linux-gnu/libasound.so.2\\n"\n' > "$bin/ldconfig"
            ;;
        absent)
            printf '#!/bin/sh\nprintf "\\tlibc.so.6 (libc6,x86-64) => /lib/x86_64-linux-gnu/libc.so.6\\n"\n' > "$bin/ldconfig"
            ;;
    esac

    if [ -n "$SIM_ROSETTA" ]; then
        cat > "$bin/sysctl" <<'EOF'
#!/bin/sh
[ "$2" = "hw.optional.arm64" ] && echo 1
EOF
    fi

    chmod +x "$bin"/*

    # The real tools the installer needs, linked after the chmod above so that
    # it only touches the fakes. gzip is for GNU tar, which runs it for -z.
    for tool in cat chmod grep gzip mkdir mv rm sed tar; do
        tool_path=$(command -v "$tool") || { echo "$tool is required"; exit 2; }
        ln -s "$tool_path" "$bin/$tool"
    done
}

run_installer() {
    make_bin
    mkdir -p "$CASE_DIR/home" "$CASE_DIR/tmp"

    case "$TERMINAL" in
        file)
            tty_path="$CASE_DIR/tty"
            if [ -n "$TTY_EOF" ]; then
                : > "$tty_path"
            else
                printf '%s\n' "$TTY_ANSWER" > "$tty_path"
            fi
            ;;
        *)
            tty_path="$CASE_DIR/no-terminal/tty"
            ;;
    esac

    if [ "$ON_PATH" -eq 1 ]; then
        case_path="$CASE_DIR/bin:$CASE_DIR/install"
    else
        case_path="$CASE_DIR/bin"
    fi

    # A file:// URL cannot hold a raw space: the temporary directory may.
    work_url=$(printf '%s' "$WORK" | sed 's/ /%20/g')
    # Never the real /usr/lib/wsl/lib/nvidia-smi: on a WSL machine with a card
    # every "no NVIDIA" case would otherwise find one.
    wsl_smi="$CASE_DIR/wsl/nvidia-smi"
    if [ -n "$SIM_WSL_NVIDIA" ]; then
        mkdir -p "$CASE_DIR/wsl"
        printf '#!/bin/sh\necho "NVIDIA-SMI 591.86 Driver Version: 591.86 CUDA Version: 12.9"\n' > "$wsl_smi"
        chmod +x "$wsl_smi"
    fi

    set -- "HOME=$CASE_DIR/home" "PATH=$case_path" "TMPDIR=$CASE_DIR/tmp" \
        "VOX_INSTALL_DIR=$CASE_DIR/install" "VOX_WSL_NVIDIA_SMI=$wsl_smi" \
        "VOX_RELEASE_BASE_URL=${BASE_URL:-file://$work_url/release-$RELEASE}"
    if [ -n "$SIM_CI" ]; then
        set -- "$@" "CI=true"
    fi
    if [ "$TERMINAL" != "pty" ]; then
        set -- "$@" "VOX_TTY=$tty_path"
    fi
    if [ "$SET_VERSION" -eq 1 ]; then
        set -- "$@" "VOX_VERSION=$TAG"
    fi
    if [ "$GPU" != "-" ]; then
        set -- "$@" "VOX_GPU=$GPU"
    fi

    STATUS=0
    if [ "$TERMINAL" = "pty" ]; then
        # The way it is really run, `curl ... | sh`, under a pseudo-terminal
        # that becomes /dev/tty. The helper types TTY_ANSWER when it sees the
        # question.
        # shellcheck disable=SC2016
        env -i "$@" "INSTALL_SH=$INSTALL_SH" "TEST_SHELL=$TEST_SHELL" \
            "$PYTHON" "$WORK/pty_run.py" "$TTY_ANSWER" /bin/sh -c 'cat "$INSTALL_SH" | "$TEST_SHELL"' \
            > "$OUT" 2>&1 || STATUS=$?
    elif [ "$RUN_MODE" = "pipe" ]; then
        # stdin is the script itself, as under `curl ... | sh`.
        # shellcheck disable=SC2002
        cat "$INSTALL_SH" | env -i "$@" "$TEST_SHELL" > "$OUT" 2>&1 || STATUS=$?
    else
        env -i "$@" "$TEST_SHELL" "$INSTALL_SH" > "$OUT" 2>&1 < /dev/null || STATUS=$?
    fi

    # Whatever happened, the installer must not leave its work files behind.
    leftovers=$(ls -A "$CASE_DIR/tmp")
    if [ -n "$leftovers" ]; then
        fail "temporary files left behind: $leftovers"
    fi
}

expect_success() {
    [ "$STATUS" -eq 0 ] || fail "expected exit status 0, got $STATUS"
}

expect_failure() {
    [ "$STATUS" -ne 0 ] || fail "expected a failure, got exit status 0"
}

expect_out() {
    grep -F -q -- "$1" "$OUT" || fail "output lacks: $1"
}

expect_no_out() {
    if grep -F -q -- "$1" "$OUT"; then
        fail "output should not contain: $1"
    fi
}

# The text must come after "Installation complete": the installer's last lines.
expect_last_lines() {
    sed -n '/Installation complete/,$p' "$OUT" | grep -F -q -- "$1" || fail "last lines lack: $1"
}

# The installed vox must be the fake built with this marker, and executable.
expect_installed() {
    installed="$CASE_DIR/install/vox"
    if [ ! -f "$installed" ]; then
        fail "nothing was installed (expected the $1 build)"
    elif ! grep -q -x "# build: $1" "$installed"; then
        fail "expected the $1 build, found: $(grep '^# build:' "$installed")"
    elif [ ! -x "$installed" ]; then
        fail "the installed vox is not executable"
    fi
}

expect_nothing_installed() {
    [ ! -e "$CASE_DIR/install/vox" ] || fail "vox was installed: $(grep '^# build:' "$CASE_DIR/install/vox")"
}

# expect_fetched "A B": exactly these assets (last part of each URL, without
# .tar.gz) were asked for, in this order. "" means curl was never called.
expect_fetched() {
    fetched=$(sed -e 's|.*/||' -e 's|\.tar\.gz$||' "$CURL_LOG" | tr '\n' ' ' | sed 's/ $//')
    [ "$fetched" = "$1" ] || fail "expected downloads [$1], got [$fetched]"
}

# The question was put on the terminal, with what the contract says it tells.
expect_question() {
    for text in "NVIDIA card was detected" "voice cloning (qwen-native" "transcription (Whisper)" \
        "default voices run on the CPU" "NVIDIA driver" "CUDA 12 runtime" "[y/N]"; do
        grep -F -q -- "$text" "$1" || fail "the question lacks: $text"
    done
}

# ------------------------------------------------------------------------ macOS

begin "Apple Silicon: Metal build, and says so"
SIM_KERNEL="Darwin"; SIM_MACHINE="arm64"
run_installer
expect_success
expect_installed metal
expect_fetched "$MAC_ARM"
expect_out "this build uses Metal"
expect_out "Verification: vox 9.9.9"
expect_last_lines "Build installed: Metal (Apple GPU)"
expect_no_out "VOX_GPU"
expect_no_out "[WARN]"
end

begin "Apple Silicon, VOX_GPU=cuda: warning, Metal build"
SIM_KERNEL="Darwin"; SIM_MACHINE="arm64"; GPU="cuda"
run_installer
expect_success
expect_installed metal
expect_fetched "$MAC_ARM"
expect_out "VOX_GPU=cuda does not apply on macOS: CUDA is for NVIDIA cards"
expect_last_lines "Build installed: Metal (Apple GPU)"
end

begin "Apple Silicon, VOX_GPU=cpu: warning, Metal build (the only macOS build)"
SIM_KERNEL="Darwin"; SIM_MACHINE="arm64"; GPU="cpu"
run_installer
expect_success
expect_installed metal
expect_out "VOX_GPU=cpu does not apply on macOS"
expect_last_lines "Build installed: Metal (Apple GPU)"
end

begin "Intel Mac: refused before any download"
SIM_KERNEL="Darwin"; SIM_MACHINE="x86_64"; SET_VERSION=0
run_installer
expect_failure
expect_nothing_installed
expect_fetched ""
expect_out "Intel Macs are not supported"
expect_out "ONNX runtime used by the piper voices has no prebuilt Intel Mac binary"
expect_no_out "Downloading"
end

begin "Apple Silicon seen as x86_64 from a Rosetta shell: native arm64 build"
SIM_KERNEL="Darwin"; SIM_MACHINE="x86_64"; SIM_ROSETTA=1
run_installer
expect_success
expect_installed metal
expect_fetched "$MAC_ARM"
expect_out "Rosetta"
end

# ----------------------------------------------------------------- Linux x86_64

begin "Linux x86_64 without NVIDIA: CPU build, no question, no CUDA talk"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_X86"
expect_last_lines "Build installed: CPU."
expect_no_out "CUDA"
expect_no_out "NVIDIA"
expect_no_out "[WARN]"
end

begin "Linux x86_64, nvidia-smi present but failing: treated as no NVIDIA card"
SIM_NVIDIA="failing"; TERMINAL="file"; TTY_ANSWER="y"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_X86"
expect_no_out "CUDA"
end

begin "Linux x86_64 with NVIDIA, VOX_GPU=cpu: CPU build, and how to switch"
SIM_NVIDIA="ok"; GPU="cpu"; TERMINAL="file"; TTY_ANSWER="y"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_X86"
expect_last_lines "Build installed: CPU."
expect_last_lines "This machine has an NVIDIA card."
expect_last_lines "| VOX_GPU=cuda VOX_INSTALL_DIR=\"$CASE_DIR/install\" sh"
end

begin "Linux x86_64 with NVIDIA, VOX_GPU=cuda: CUDA build, no question"
SIM_NVIDIA="ok"; GPU="cuda"; TERMINAL="file"; TTY_ANSWER="n"
run_installer
expect_success
expect_installed cuda
expect_fetched "$LINUX_CUDA"
expect_last_lines "Build installed: CUDA (NVIDIA GPU)"
expect_no_out "[WARN]"
[ "$(cat "$CASE_DIR/tty")" = "n" ] || fail "the terminal was written to although VOX_GPU was set"
end

begin "Linux x86_64 with NVIDIA, auto, no terminal: CPU build plus the hint"
SIM_NVIDIA="ok"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_X86"
expect_out "nobody to ask (no terminal, or a CI job): installing the CPU build. For the CUDA build, run the installer with VOX_GPU=cuda."
expect_last_lines "Build installed: CPU."
expect_last_lines "| VOX_GPU=cuda"
[ ! -e "$CASE_DIR/no-terminal" ] || fail "probing for a terminal created $CASE_DIR/no-terminal"
end

begin "Linux x86_64 with NVIDIA, auto, no terminal, run as a pipe: same"
SIM_NVIDIA="ok"; RUN_MODE="pipe"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_X86"
expect_out "nobody to ask"
end

# The question. The terminal is the VOX_TTY file: the answer is its first line
# and the installer appends the question to it. The script arrives on stdin, as
# under `curl ... | sh`, so an answer read from stdin instead of the terminal
# would show up here as the wrong build.
for answer in y Y yes YES Yes; do
    begin "question answered '$answer': CUDA build"
    SIM_NVIDIA="ok"; TERMINAL="file"; TTY_ANSWER="$answer"; RUN_MODE="pipe"
    run_installer
    expect_success
    expect_installed cuda
    expect_fetched "$LINUX_CUDA"
    expect_question "$CASE_DIR/tty"
    expect_last_lines "Build installed: CUDA (NVIDIA GPU)"
    end
done

for answer in n N no "" maybe yy "y es"; do
    begin "question answered '$answer': CPU build (default is No)"
    SIM_NVIDIA="ok"; TERMINAL="file"; TTY_ANSWER="$answer"; RUN_MODE="pipe"
    run_installer
    expect_success
    expect_installed cpu-x86_64
    expect_fetched "$LINUX_X86"
    expect_question "$CASE_DIR/tty"
    expect_last_lines "Build installed: CPU."
    expect_last_lines "| VOX_GPU=cuda"
    end
done

begin "question, terminal closes without an answer: CPU build"
SIM_NVIDIA="ok"; TERMINAL="file"; TTY_EOF=1; RUN_MODE="pipe"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_X86"
end

begin "CUDA asset missing from the release: warning, CPU build"
SIM_NVIDIA="ok"; GPU="cuda"; RELEASE="nocuda"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_CUDA $LINUX_X86"
expect_out "Release $TAG has no CUDA build."
expect_out "Installing the CPU build instead."
expect_last_lines "Build installed: CPU."
expect_last_lines "release $TAG does not have one"
end

begin "CUDA build does not start: missing libraries listed, CPU build"
SIM_NVIDIA="ok"; GPU="cuda"; RELEASE="brokencuda"; SIM_LDD="missing"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_CUDA $LINUX_X86"
expect_out "The CUDA build does not start on this machine."
# What the binary itself said, not only a guess at the cause.
expect_out "It said:"
expect_out "error while loading shared libraries: libcublas.so.12"
expect_out "Missing libraries:"
expect_out "libcublas.so.12 => not found"
expect_out "libcuda.so.1 => not found"
expect_no_out "libc.so.6"
expect_out "Installing the CPU build instead."
expect_last_lines "Build installed: CPU."
expect_last_lines "it does not start here"
end

begin "CUDA build does not start, no ldd: still falls back to the CPU build"
SIM_NVIDIA="ok"; GPU="cuda"; RELEASE="brokencuda"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_CUDA $LINUX_X86"
expect_out "The CUDA build does not start on this machine."
expect_out "ldd is not available"
expect_out "NVIDIA driver (libcuda.so.1) and the CUDA 12 runtime libraries cuBLAS"
end

begin "question answered yes, CUDA build does not start: CPU build"
SIM_NVIDIA="ok"; TERMINAL="file"; TTY_ANSWER="y"; RELEASE="brokencuda"; RUN_MODE="pipe"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_CUDA $LINUX_X86"
expect_last_lines "Build installed: CPU."
end

begin "CUDA asset is not an archive: warning, CPU build"
SIM_NVIDIA="ok"; GPU="cuda"; RELEASE="corruptcuda"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_CUDA $LINUX_X86"
expect_out "Could not get the CUDA build"
expect_last_lines "Build installed: CPU."
end

begin "VOX_GPU=cuda on Linux x86_64 without nvidia-smi: warning, CUDA build still tried"
GPU="cuda"
run_installer
expect_success
expect_installed cuda
expect_fetched "$LINUX_CUDA"
expect_out "no NVIDIA card was found (nvidia-smi is missing or failed)"
expect_last_lines "Build installed: CUDA (NVIDIA GPU)"
end

begin "VOX_GPU=cuda without nvidia-smi, CUDA build does not start: CPU build"
GPU="cuda"; RELEASE="brokencuda"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_CUDA $LINUX_X86"
expect_last_lines "it does not start here"
end

begin "WSL: nvidia-smi off PATH, only where WSL keeps it: the card is still found"
SIM_WSL_NVIDIA=1
run_installer
expect_success
expect_installed cpu-x86_64
expect_out "NVIDIA card detected, and nobody to ask"
expect_last_lines "| VOX_GPU=cuda"
end

begin "WSL: nvidia-smi off PATH, VOX_GPU=cuda: CUDA build, no 'no card' warning"
SIM_WSL_NVIDIA=1; GPU="cuda"
run_installer
expect_success
expect_installed cuda
expect_no_out "no NVIDIA card was found"
end

begin "NVIDIA card older than the CUDA build supports, terminal: no question, CPU build, and why"
SIM_NVIDIA="old"; TERMINAL="file"; TTY_ANSWER="y"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_X86"
[ "$(cat "$CASE_DIR/tty")" = "y" ] || fail "a question was asked for a card that cannot run the build"
expect_out "compute capability 6.1. The CUDA build needs 8.0 or newer"
expect_last_lines "is older than the CUDA build supports"
expect_no_out "| VOX_GPU=cuda"
end

begin "VOX_GPU=cuda on a card older than the build supports: CPU build, not a binary that fails later"
SIM_NVIDIA="old"; GPU="cuda"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_X86"
expect_out "compute capability 6.1. The CUDA build needs 8.0 or newer"
end

begin "driver too old to report the card's generation: treated as unknown, the question is asked"
SIM_NVIDIA="noquery"; TERMINAL="file"; TTY_ANSWER="y"; RUN_MODE="pipe"
run_installer
expect_success
expect_installed cuda
expect_question "$CASE_DIR/tty"
end

begin "Linux with glibc older than the binaries need: refused before any download"
SIM_GLIBC="glibc 2.35"
run_installer
expect_failure
expect_nothing_installed
expect_fetched ""
expect_out "vox needs glibc 2.39 or newer"
expect_out "this system has glibc 2.35"
end

for libc in "glibc 2.39" "glibc 2.43" "glibc 3.0" "musl" ""; do
    begin "Linux reporting '$libc': not refused on the C library"
    SIM_GLIBC="$libc"
    run_installer
    expect_success
    expect_installed cpu-x86_64
    end
done

begin "macOS never checks glibc"
SIM_KERNEL="Darwin"; SIM_MACHINE="arm64"; SIM_GLIBC="glibc 2.10"
run_installer
expect_success
expect_installed metal
end

begin "CI job with a terminal: nobody to answer, so no question and the CPU build"
SIM_NVIDIA="ok"; SIM_CI=1; TERMINAL="file"; TTY_ANSWER="y"
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "$LINUX_X86"
[ "$(cat "$CASE_DIR/tty")" = "y" ] || fail "a question was asked in a CI job"
expect_out "NVIDIA card detected, and nobody to ask"
end

# ------------------------------------------------------------------ Linux arm64

begin "VOX_GPU=cuda on Linux arm64: warning, CPU build"
SIM_MACHINE="aarch64"; SIM_NVIDIA="ok"; GPU="cuda"
run_installer
expect_success
expect_installed cpu-arm64
expect_fetched "$LINUX_ARM"
expect_out "VOX_GPU=cuda does not apply here: the CUDA build exists only for Linux x86_64"
expect_last_lines "Build installed: CPU."
expect_last_lines "the CUDA build exists only for Linux x86_64"
expect_no_out "| VOX_GPU=cuda"
end

begin "Linux arm64 with NVIDIA, auto, terminal: no question, CPU build"
SIM_MACHINE="aarch64"; SIM_NVIDIA="ok"; TERMINAL="file"; TTY_ANSWER="y"
run_installer
expect_success
expect_installed cpu-arm64
expect_fetched "$LINUX_ARM"
[ "$(cat "$CASE_DIR/tty")" = "y" ] || fail "a question was asked on Linux arm64"
end

# ---------------------------------------------------------------- other outcomes

for value in gpu CUDA metal 1; do
    begin "VOX_GPU=$value: error naming the three values, nothing downloaded"
    SIM_NVIDIA="ok"; GPU="$value"; SET_VERSION=0
    run_installer
    expect_failure
    expect_nothing_installed
    expect_fetched ""
    expect_out "Invalid VOX_GPU value '$value'. Use one of: auto, cuda, cpu."
    end
done

begin "unsupported OS: refused before any download"
SIM_KERNEL="MINGW64_NT-10.0"; SET_VERSION=0
run_installer
expect_failure
expect_nothing_installed
expect_fetched ""
expect_out "Unsupported OS: MINGW64_NT-10.0"
end

begin "no VOX_VERSION: the tag comes from the GitHub API answer"
SET_VERSION=0
run_installer
expect_success
expect_installed cpu-x86_64
expect_fetched "latest $LINUX_X86"
expect_out "Version: $TAG"
end

begin "release without the asset for this platform: error, nothing installed"
RELEASE="empty"
run_installer
expect_failure
expect_nothing_installed
expect_out "Failed to get the vox binary"
end

begin "Linux without ALSA: the installer still warns about it"
SIM_ALSA="absent"
run_installer
expect_success
expect_installed cpu-x86_64
expect_out "ALSA not found, and vox needs it to start. Install it: sudo apt install libasound2t64"
end

begin "macOS: no ALSA check"
SIM_KERNEL="Darwin"; SIM_MACHINE="arm64"; SIM_ALSA="unknown"
run_installer
expect_success
expect_no_out "ALSA"
end

begin "install directory not on PATH: the installer still says so"
ON_PATH=0
run_installer
expect_success
expect_installed cpu-x86_64
expect_out "Binary installed but not in PATH. Add $CASE_DIR/install to your PATH."
end

begin "installed binary does not start: said plainly, and the installer fails"
RELEASE="brokencpu"; SIM_LDD="missing"
run_installer
expect_failure
expect_installed cpu-x86_64
expect_out "is installed but does not start"
expect_out "It said:"
expect_out "libcublas.so.12 => not found"
expect_out "does not start on this machine"
expect_no_out "Installation complete"
end

begin "installed binary needs a newer glibc: says so and points to a source build"
RELEASE="oldglibc"
run_installer
expect_failure
expect_out "version GLIBC_2.39 not found"
expect_out "glibc) is older than vox needs: glibc 2.39 or newer"
expect_no_out "Installation complete"
end

# ------------------------------------------------- needs python3: HTTP and a pty

if [ -n "$PYTHON" ]; then
    # A real HTTP 404, from a server on the loopback interface.
    cat > "$WORK/serve.py" <<'EOF'
import functools, http.server, sys
handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=sys.argv[1])
server = http.server.HTTPServer(("127.0.0.1", 0), handler)
with open(sys.argv[2], "w") as port_file:
    port_file.write(str(server.server_address[1]))
server.serve_forever()
EOF
    "$PYTHON" -u "$WORK/serve.py" "$WORK/release-nocuda" "$WORK/port" > "$WORK/serve.log" 2>&1 &
    SERVER_PID=$!
    tries=0
    while [ ! -s "$WORK/port" ] && [ "$tries" -lt 50 ]; do
        sleep 0.1
        tries=$((tries + 1))
    done
fi

if [ -n "$PYTHON" ] && [ -s "$WORK/port" ]; then
    begin "CUDA asset missing over HTTP (404): warning, CPU build"
    SIM_NVIDIA="ok"; GPU="cuda"; BASE_URL="http://127.0.0.1:$(cat "$WORK/port")"
    run_installer
    expect_success
    expect_installed cpu-x86_64
    expect_fetched "$LINUX_CUDA $LINUX_X86"
    expect_out "Release $TAG has no CUDA build."
    grep -q "$LINUX_CUDA.tar.gz HTTP/1.1\" 404" "$WORK/serve.log" || fail "the server did not answer 404 for the CUDA asset"
    end
else
    skip "CUDA asset missing over HTTP (404): needs python3 and a local port"
fi
stop_server

if [ -n "$PYTHON" ]; then
    # Runs a command under a pseudo-terminal, which becomes its /dev/tty, and
    # types the answer once the question shows up.
    cat > "$WORK/pty_run.py" <<'EOF'
import os, pty, select, sys, time
answer, command = sys.argv[1], sys.argv[2:]
pid, fd = pty.fork()
if pid == 0:
    os.execvp(command[0], command)
seen, answered, deadline = b"", False, time.time() + 60
while time.time() < deadline:
    readable, _, _ = select.select([fd], [], [], 1)
    if not readable:
        continue
    try:
        data = os.read(fd, 4096)
    except OSError:
        break
    if not data:
        break
    seen += data
    if not answered and b"[y/N]" in seen:
        os.write(fd, answer.encode() + b"\n")
        answered = True
_, status = os.waitpid(pid, 0)
sys.stdout.buffer.write(seen)
sys.stdout.flush()
if not answered:
    sys.exit(97)
sys.exit(os.WEXITSTATUS(status) if os.WIFEXITED(status) else 98)
EOF
    if "$PYTHON" "$WORK/pty_run.py" "" /bin/sh -c '( : < /dev/tty ) && printf "[y/N]" > /dev/tty && read -r line < /dev/tty' > /dev/null 2>&1; then
        begin "real terminal (pty), curl | sh, 'y' typed at the question: CUDA build"
        SIM_NVIDIA="ok"; TERMINAL="pty"; TTY_ANSWER="y"
        run_installer
        expect_success
        expect_installed cuda
        expect_fetched "$LINUX_CUDA"
        expect_question "$OUT"
        expect_last_lines "Build installed: CUDA (NVIDIA GPU)"
        end

        begin "real terminal (pty), curl | sh, Enter at the question: CPU build"
        SIM_NVIDIA="ok"; TERMINAL="pty"; TTY_ANSWER=""
        run_installer
        expect_success
        expect_installed cpu-x86_64
        expect_fetched "$LINUX_X86"
        expect_question "$OUT"
        expect_last_lines "Build installed: CPU."
        end
    else
        skip "real terminal (pty): no pseudo-terminal available here"
    fi
else
    skip "real terminal (pty): needs python3"
fi

# ---------------------------------------------------------------------- summary

echo ""
echo "$PASSED passed, $FAILED failed, $SKIPPED skipped (install.sh run by $TEST_SHELL)"
[ "$FAILED" -eq 0 ]
