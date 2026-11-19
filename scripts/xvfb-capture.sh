#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'U'
Usage: scripts/xvfb-capture.sh --core CORE.so --rom ROM [options] --out SHOT.png

Runs emu-rust under a private Xvfb, captures the screen as PNG, then stops
both Xvfb and emu-rust (by PID, never by name).

Required:
  --core PATH       libretro core (.so)
  --rom PATH        ROM file
  --out PATH        output PNG (default: target/test-output/xvfb.png)
Options:
  --frames N        passed to emu-rust --frames (default: 600)
  --delay SECS      wait before capturing (default: 3)
  --screen WxHxD    Xvfb screen (default: 1280x800x24)
  --bin PATH        use this emu-rust binary instead of building target/debug/emu-rust
  --                everything after is passed to emu-rust unchanged
  -h, --help        this help
Needs: Xvfb, xwd, convert (ImageMagick). Software GL is forced (LIBGL_ALWAYS_SOFTWARE=1).
The emu must still be running at capture time: raise --frames or lower --delay.
U
}

core="" rom="" out="target/test-output/xvfb.png" frames=600 delay=3
screen="1280x800x24" bin="" extra=()
while (($#)); do
    case "$1" in
        --core) core=${2:?}; shift 2 ;;
        --rom) rom=${2:?}; shift 2 ;;
        --out) out=${2:?}; shift 2 ;;
        --frames) frames=${2:?}; shift 2 ;;
        --delay) delay=${2:?}; shift 2 ;;
        --screen) screen=${2:?}; shift 2 ;;
        --bin) bin=${2:?}; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        --) shift; extra=("$@"); break ;;
        *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done
[[ -n $core && -n $rom ]] || { echo "--core and --rom are required" >&2; usage >&2; exit 2; }
[[ -f $core ]] || { echo "core not found: $core (run scripts/fetch-test-assets.sh)" >&2; exit 1; }
[[ -f $rom ]] || { echo "rom not found: $rom (run scripts/fetch-test-assets.sh)" >&2; exit 1; }
for t in Xvfb xwd convert; do
    command -v "$t" >/dev/null || { echo "missing tool: $t" >&2; exit 1; }
done

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
if [[ -z $bin ]]; then
    cargo build -p emu-app
    bin="$root/target/debug/emu-rust"
fi
[[ -x $bin ]] || { echo "binary not executable: $bin" >&2; exit 1; }
mkdir -p "$(dirname "$out")"

tmp=$(mktemp -d)
xpid="" epid=""
cleanup() {
    for p in $epid $xpid; do
        if kill -0 "$p" 2>/dev/null; then
            kill "$p" 2>/dev/null || true
            wait "$p" 2>/dev/null || true
        fi
    done
    rm -rf "$tmp"
}
trap cleanup EXIT

# -displayfd lets Xvfb pick a free display number: no clash with other servers.
Xvfb -displayfd 3 -screen 0 "$screen" -nolisten tcp 3>"$tmp/display" 2>"$tmp/xvfb.log" &
xpid=$!
for _ in $(seq 100); do [[ -s $tmp/display ]] && break; sleep 0.1; done
[[ -s $tmp/display ]] || { echo "Xvfb did not start:" >&2; cat "$tmp/xvfb.log" >&2; exit 1; }
export DISPLAY=":$(head -n1 "$tmp/display")"

env -u WAYLAND_DISPLAY LIBGL_ALWAYS_SOFTWARE=1 \
    "$bin" --core "$core" --rom "$rom" --frames "$frames" "${extra[@]}" &
epid=$!

sleep "$delay"
kill -0 "$epid" 2>/dev/null || { echo "emu-rust exited before capture (raise --frames or lower --delay)" >&2; wait "$epid" || true; epid=""; exit 1; }
xwd -root -silent -display "$DISPLAY" | convert xwd:- "png:$out"
echo "captured $out on $DISPLAY"
