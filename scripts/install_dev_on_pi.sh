#!/usr/bin/env sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

ROOT_DIR=/
ENABLE_UNITS=1

ensure_runtime_packages() {
    if [ "$ROOT_DIR" != "/" ] || [ "$(uname -s)" != "Linux" ]; then
        return 0
    fi

    if ! command -v apt-get >/dev/null 2>&1 || ! command -v dpkg-query >/dev/null 2>&1; then
        return 0
    fi

    missing_packages=
    for package in hostapd dnsmasq avahi-daemon; do
        if ! dpkg-query -W -f='${Status}' "$package" 2>/dev/null | grep -q "install ok installed"; then
            missing_packages="${missing_packages} ${package}"
        fi
    done

    if [ -n "$missing_packages" ]; then
        apt-get update
        # shellcheck disable=SC2086
        apt-get install -y $missing_packages
    fi
}

usage() {
    cat <<'EOF'
Usage: scripts/install_dev_on_pi.sh [--root PATH] [--no-enable]

Installs the current ClawPi proving-ground binaries and systemd units.

Options:
  --root PATH   stage the install into PATH instead of /
  --no-enable   skip enabling clawpi-mode.service
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --root)
            if [ "$#" -lt 2 ]; then
                echo "missing value for --root" >&2
                exit 2
            fi
            ROOT_DIR=$2
            shift 2
            ;;
        --no-enable)
            ENABLE_UNITS=0
            shift
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "unsupported argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [ "$ROOT_DIR" = "/" ] && [ "$(uname -s)" != "Linux" ]; then
    echo "install_dev_on_pi.sh targets a Linux root. Use --root when staging from macOS." >&2
    exit 1
fi

ensure_runtime_packages

cd "$REPO_ROOT"
cargo build --release

LIBEXEC_DIR=$ROOT_DIR/usr/local/libexec/clawpi
BIN_DIR=$ROOT_DIR/usr/local/bin
SYSTEMD_DIR=$ROOT_DIR/etc/systemd/system
CONFIG_DIR=$ROOT_DIR/etc/clawpi
STATE_DIR=$ROOT_DIR/var/lib/clawpi

install -d "$LIBEXEC_DIR" "$BIN_DIR" "$SYSTEMD_DIR" "$CONFIG_DIR" "$STATE_DIR"

install -m 0755 target/release/clawpi-init "$LIBEXEC_DIR/clawpi-init"
install -m 0755 target/release/clawpi-recoveryd "$LIBEXEC_DIR/clawpi-recoveryd"
install -m 0755 target/release/clawpi-setupd "$LIBEXEC_DIR/clawpi-setupd"
install -m 0755 target/release/clawpi-sessiond "$LIBEXEC_DIR/clawpi-sessiond"
install -m 0755 target/release/clawpi-agentd "$LIBEXEC_DIR/clawpi-agentd"
install -m 0755 target/release/clawpi-wifid "$LIBEXEC_DIR/clawpi-wifid"
install -m 0755 target/release/clawpi-webd "$LIBEXEC_DIR/clawpi-webd"
install -m 0755 target/release/clawpi-portald "$LIBEXEC_DIR/clawpi-portald"
install -m 0755 target/release/clawpi-ctl "$BIN_DIR/clawpi-ctl"

install -m 0644 systemd/clawpi.target "$SYSTEMD_DIR/clawpi.target"
install -m 0644 systemd/clawpi-setup.target "$SYSTEMD_DIR/clawpi-setup.target"
install -m 0644 systemd/clawpi-recovery.target "$SYSTEMD_DIR/clawpi-recovery.target"
install -m 0644 systemd/clawpi-mode.service "$SYSTEMD_DIR/clawpi-mode.service"
install -m 0644 systemd/clawpi-recoveryd.service "$SYSTEMD_DIR/clawpi-recoveryd.service"
install -m 0644 systemd/clawpi-portald.service "$SYSTEMD_DIR/clawpi-portald.service"
install -m 0644 systemd/clawpi-setupd.service "$SYSTEMD_DIR/clawpi-setupd.service"
install -m 0644 systemd/clawpi-sessiond.service "$SYSTEMD_DIR/clawpi-sessiond.service"
install -m 0644 systemd/clawpi-agentd.service "$SYSTEMD_DIR/clawpi-agentd.service"
install -m 0644 systemd/clawpi-wifid.service "$SYSTEMD_DIR/clawpi-wifid.service"
install -m 0644 systemd/clawpi-webd.service "$SYSTEMD_DIR/clawpi-webd.service"

ln -snf /dev/null "$SYSTEMD_DIR/hostapd.service"
ln -snf /dev/null "$SYSTEMD_DIR/dnsmasq.service"

if [ "$ROOT_DIR" = "/" ]; then
    "$LIBEXEC_DIR/clawpi-setupd" --once
else
    CLAWPI_ROOT="$ROOT_DIR" "$LIBEXEC_DIR/clawpi-setupd" --once
fi

if [ "$ENABLE_UNITS" -eq 1 ]; then
    if [ "$ROOT_DIR" = "/" ] && command -v systemctl >/dev/null 2>&1; then
        systemctl daemon-reload
        systemctl enable clawpi-mode.service
        systemctl enable avahi-daemon.service
    else
        WANTS_DIR=$SYSTEMD_DIR/multi-user.target.wants
        install -d "$WANTS_DIR"
        ln -snf ../clawpi-mode.service "$WANTS_DIR/clawpi-mode.service"
    fi
fi

echo "clawpi: installed proving-ground artifacts into $ROOT_DIR"
echo "clawpi: binaries:"
echo "  $LIBEXEC_DIR/clawpi-init"
echo "  $LIBEXEC_DIR/clawpi-recoveryd"
echo "  $LIBEXEC_DIR/clawpi-setupd"
echo "  $LIBEXEC_DIR/clawpi-sessiond"
echo "  $LIBEXEC_DIR/clawpi-agentd"
echo "  $LIBEXEC_DIR/clawpi-wifid"
echo "  $LIBEXEC_DIR/clawpi-webd"
echo "  $LIBEXEC_DIR/clawpi-portald"
echo "  $BIN_DIR/clawpi-ctl"
echo "clawpi: systemd units:"
echo "  $SYSTEMD_DIR/clawpi-mode.service"
echo "  $SYSTEMD_DIR/clawpi-portald.service"
echo "  $SYSTEMD_DIR/clawpi-recoveryd.service"
echo "  $SYSTEMD_DIR/clawpi-sessiond.service"
echo "  $SYSTEMD_DIR/clawpi-agentd.service"
echo "  $SYSTEMD_DIR/clawpi-setupd.service"
echo "  $SYSTEMD_DIR/clawpi-wifid.service"
echo "  $SYSTEMD_DIR/clawpi-webd.service"
echo "  $SYSTEMD_DIR/clawpi.target"
echo "  $SYSTEMD_DIR/clawpi-setup.target"
echo "  $SYSTEMD_DIR/clawpi-recovery.target"
echo "clawpi: masked distro services:"
echo "  $SYSTEMD_DIR/hostapd.service"
echo "  $SYSTEMD_DIR/dnsmasq.service"
echo "clawpi: setup contract:"
echo "  $CONFIG_DIR/config.toml"

if [ "$ROOT_DIR" = "/" ]; then
    echo "clawpi: next steps on the Pi:"
    echo "  systemctl start clawpi-mode.service"
    echo "  clawpi-ctl status"
else
    echo "clawpi: next step for staged verification:"
    echo "  CLAWPI_ROOT=$ROOT_DIR target/release/clawpi-ctl status"
fi
