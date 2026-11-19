#!/usr/bin/env bash
# Audio diagnostic: a sine beep on the left then the right channel through the same ALSA -> PulseAudio path as emu-rust.
set -euo pipefail

if [[ -z "${PULSE_SERVER:-}" ]]; then
    echo "PULSE_SERVER is not set: run the wslg or x11 service (the novnc mode has no sound)." >&2
    exit 1
fi
# Only unix sockets can be checked locally; tcp:... is left to speaker-test.
pulse_path="${PULSE_SERVER#unix:}"
if [[ "$pulse_path" == /* && ! -S "$pulse_path" ]]; then
    echo "PulseAudio socket $pulse_path not found (is /mnt/wslg mounted?)." >&2
    exit 1
fi
printf 'pcm.!default pulse\nctl.!default pulse\n' > "$HOME/.asoundrc"
exec speaker-test -c2 -t sine -f 440 -l1
