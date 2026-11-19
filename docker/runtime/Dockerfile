# Runtime image: a deployment target like the native builds (ADR 0004).
# Build context: repository root. Example: docker compose -f docker/compose.yaml build

FROM rust:1.100-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config cmake clang \
        libx11-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
        libgl-dev libegl-dev libasound2-dev libpulse-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN cargo build --release -p emu-app

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        libx11-6 libxcursor1 libxrandr2 libxi6 libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libwayland-egl1 \
        libgl1 libegl1 libglx-mesa0 libegl-mesa0 libgl1-mesa-dri \
        xkb-data \
        libasound2 libasound2-plugins libpulse0 \
        # noVNC mode only; small enough to keep a single image.
        xvfb x11vnc novnc websockify \
    && rm -rf /var/lib/apt/lists/*

# ALSA -> PulseAudio so WSLg / host PulseAudio receive the sound.
RUN printf 'pcm.!default pulse\nctl.!default pulse\n' > /etc/asound.conf

RUN useradd --create-home --uid 1000 emu && mkdir -p /data && chown emu:emu /data
COPY --from=build /src/target/release/emu-rust /usr/local/bin/emu-rust
COPY docker/runtime/entrypoint.sh /usr/local/bin/entrypoint.sh

USER emu
ENV EMU_DATA_DIR=/data
VOLUME ["/data"]
EXPOSE 6080
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
