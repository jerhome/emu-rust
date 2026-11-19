# Development image: mirrors the environment Claude works in (build, tests, Xvfb screenshots).
FROM rust:1.100-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config cmake clang \
        libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
        libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
        libgl-dev libegl-dev libgl1-mesa-dri mesa-utils \
        libasound2-dev libpulse-dev \
        xvfb xauth imagemagick \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy

WORKDIR /workspace/emu-rust
