# Development image: mirrors the environment Claude works in (build, tests, Xvfb screenshots).
FROM rust:1.100-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config cmake clang \
        libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
        libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
        libgl-dev libegl-dev libgl1 libegl1 libglx-mesa0 libegl-mesa0 libgl1-mesa-dri mesa-utils \
        xkb-data libxkbcommon-x11-0 \
        libasound2-dev libpulse-dev \
        xvfb xauth imagemagick x11-utils x11-apps \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy

WORKDIR /workspace/emu-rust
