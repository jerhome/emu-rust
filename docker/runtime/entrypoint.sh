#!/usr/bin/env bash
# EMU_DISPLAY=host (default): use the display/audio passed in by compose (WSLg, X11 or Wayland).
# EMU_DISPLAY=novnc: start a virtual screen reachable at http://<host>:6080/vnc.html (no sound).
set -euo pipefail

if [[ "${EMU_DISPLAY:-host}" == "novnc" ]]; then
    export DISPLAY=:99
    unset WAYLAND_DISPLAY
    Xvfb :99 -screen 0 "${EMU_RESOLUTION:-1280x720x24}" -nolisten tcp &
    for _ in $(seq 50); do [[ -e /tmp/.X11-unix/X99 ]] && break; sleep 0.1; done
    x11vnc -display :99 -forever -shared -nopw -localhost -quiet &
    websockify --web /usr/share/novnc 6080 localhost:5900 &
fi

exec /usr/local/bin/emu-rust "$@"
