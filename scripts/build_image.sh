#!/usr/bin/env sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

OUT_DIR=$REPO_ROOT/target/pi-gen
PI_GEN_DIR=

usage() {
    cat <<'EOF'
Usage: scripts/build_image.sh [--out PATH] [--pi-gen-dir PATH]

Assembles the current ClawPi pi-gen stage bundle and optionally runs pi-gen.

Options:
  --out PATH         output directory for the assembled pi-gen stage bundle
  --pi-gen-dir PATH  existing pi-gen checkout to sync and run after assembling the stage
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --out)
            if [ "$#" -lt 2 ]; then
                echo "missing value for --out" >&2
                exit 2
            fi
            OUT_DIR=$2
            shift 2
            ;;
        --pi-gen-dir)
            if [ "$#" -lt 2 ]; then
                echo "missing value for --pi-gen-dir" >&2
                exit 2
            fi
            PI_GEN_DIR=$2
            shift 2
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

STAGE_DIR=$OUT_DIR/stage-clawpi
STAGE_STEP_DIR=$STAGE_DIR/01-clawpi
FILES_DIR=$STAGE_STEP_DIR/files
TEMPLATE_DIR=$REPO_ROOT/image/pi-gen/stage-clawpi

rm -rf "$STAGE_DIR"
install -d "$STAGE_STEP_DIR"

cp "$TEMPLATE_DIR/EXPORT_IMAGE" "$STAGE_DIR/EXPORT_IMAGE"
cp "$TEMPLATE_DIR/README.md" "$STAGE_DIR/README.md"
cp "$TEMPLATE_DIR/01-clawpi/00-run.sh" "$STAGE_STEP_DIR/00-run.sh"
cp "$TEMPLATE_DIR/01-clawpi/00-run-chroot.sh" "$STAGE_STEP_DIR/00-run-chroot.sh"
chmod 0755 "$STAGE_STEP_DIR/00-run.sh" "$STAGE_STEP_DIR/00-run-chroot.sh"

"$REPO_ROOT/scripts/install_dev_on_pi.sh" --root "$FILES_DIR" --no-enable

echo "clawpi: assembled pi-gen stage bundle:"
echo "  $STAGE_DIR"
echo "clawpi: stage entry:"
echo "  $STAGE_STEP_DIR"
echo "clawpi: stage files root:"
echo "  $FILES_DIR"

if [ -n "$PI_GEN_DIR" ]; then
    if [ ! -x "$PI_GEN_DIR/build.sh" ]; then
        echo "pi-gen build.sh not found in $PI_GEN_DIR" >&2
        exit 1
    fi

    PI_GEN_STAGE_DIR=$PI_GEN_DIR/stage-clawpi
    rm -rf "$PI_GEN_STAGE_DIR"
    cp -R "$STAGE_DIR" "$PI_GEN_STAGE_DIR"

    cat >"$PI_GEN_DIR/config" <<EOF
IMG_NAME='clawpi'
RELEASE='trixie'
ENABLE_SSH=1
STAGE_LIST="stage0 stage1 stage2 stage-clawpi"
EOF

    echo "clawpi: synced custom stage into pi-gen:"
    echo "  $PI_GEN_STAGE_DIR"
    echo "clawpi: wrote pi-gen config:"
    echo "  $PI_GEN_DIR/config"
    echo "clawpi: running pi-gen with STAGE_LIST:"
    echo "  stage0 stage1 stage2 stage-clawpi"
    (
        cd "$PI_GEN_DIR"
        ./build.sh
    )
else
    echo "clawpi: next step to build with pi-gen:"
    echo "  sh ./scripts/build_image.sh --pi-gen-dir /path/to/pi-gen"
fi
