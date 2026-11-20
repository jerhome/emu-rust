#!/usr/bin/env bash
# No argument: emu-rust opens the game library (1.0); --core/--rom are never forced here.
# EMU_DISPLAY=host (default): use the display/audio passed in by compose (WSLg, X11 or Wayland).
# EMU_DISPLAY=novnc: start a virtual screen reachable at http://<host>:6080/vnc.html (no sound).
set -euo pipefail

# Data root (ADR 0011): subfolders exist even on a volume created by an older image.
data="${EMU_DATA_DIR:-/data}"
export EMU_DATA_DIR="$data"
# library.sqlite (+ -wal/-shm) lives in $data itself and is created by the app.
mkdir -p "$data/roms" "$data/system" "$data/saves" "$data/states" "$data/assets" "$data/cores" "$data/covers" "$data/database" "$data/game-options"
# config.toml holds credentials later on: keep it private (the app also enforces 0600).
if [[ -f "$data/config.toml" ]]; then chmod 600 "$data/config.toml"; fi

# ALSA default -> PulseAudio only if a server is configured; otherwise no sound, no blocking.
if [[ -n "${PULSE_SERVER:-}" ]]; then
    printf 'pcm.!default pulse\nctl.!default pulse\n' > "$HOME/.asoundrc"
else
    rm -f "$HOME/.asoundrc"
fi

# Sound-less mode: tell emu-rust not to open an audio device (avoids ALSA noise on stderr).
args=("$@")
if [[ "${EMU_DISPLAY:-host}" == "novnc" ]]; then
    args=(--no-audio "$@")
    export DISPLAY=:99
    unset WAYLAND_DISPLAY
    Xvfb :99 -screen 0 "${EMU_RESOLUTION:-1280x720x24}" -nolisten tcp &
    for _ in $(seq 50); do [[ -e /tmp/.X11-unix/X99 ]] && break; sleep 0.1; done
    x11vnc -display :99 -forever -shared -nopw -localhost -quiet &
    websockify --web /usr/share/novnc 6080 localhost:5900 &
    if [[ "${EMU_KEEP_ALIVE:-0}" == "1" ]]; then
        # Keep the virtual screen up for debugging even if emu-rust exits (e.g. a crash at startup).
        /usr/local/bin/emu-rust "${args[@]}" || true
        wait
    fi
fi

exec /usr/local/bin/emu-rust "${args[@]}"
