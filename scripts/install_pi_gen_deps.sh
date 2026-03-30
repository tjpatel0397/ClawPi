#!/usr/bin/env sh
set -eu

PI_GEN_DIR=
PRINT_ONLY=0

usage() {
    cat <<'EOF'
Usage: scripts/install_pi_gen_deps.sh [--pi-gen-dir PATH] [--print]

Installs the Debian packages needed to run pi-gen on a Linux build host.

Options:
  --pi-gen-dir PATH  optional pi-gen checkout to read package mappings from
  --print            print the apt command instead of running it
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --pi-gen-dir)
            if [ "$#" -lt 2 ]; then
                echo "missing value for --pi-gen-dir" >&2
                exit 2
            fi
            PI_GEN_DIR=$2
            shift 2
            ;;
        --print)
            PRINT_ONLY=1
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

dependency_packages() {
    if [ -n "$PI_GEN_DIR" ] && [ -f "$PI_GEN_DIR/depends" ]; then
        awk -F: '
            {
                package = ($2 != "" ? $2 : $1)
                if (package != "") {
                    print package
                }
            }
        ' "$PI_GEN_DIR/depends" | sort -u
        return
    fi

    cat <<'EOF'
coreutils
quilt
parted
qemu-user-binfmt
debootstrap
zerofree
zip
dosfstools
e2fsprogs
libarchive-tools
libcap2-bin
grep
rsync
xz-utils
file
git
curl
bc
gpg
pigz
xxd
arch-test
bmap-tools
kmod
EOF
}

PACKAGES=$(dependency_packages | tr '\n' ' ' | sed 's/[[:space:]]*$//')

if [ -z "$PACKAGES" ]; then
    echo "no pi-gen packages resolved" >&2
    exit 1
fi

if [ "$PRINT_ONLY" -eq 1 ]; then
    echo "apt-get update && apt-get install -y $PACKAGES"
    exit 0
fi

if [ "$(id -u)" -ne 0 ]; then
    echo "install_pi_gen_deps.sh must run as root unless you use --print" >&2
    exit 1
fi

apt-get update
apt-get install -y $PACKAGES
