# Linux environment for scripts/check_all.py and the device tests: the Rust
# toolchain of rust-toolchain.toml, GCC, CMake, ALSA headers and a
# PulseAudio server for a null sink.
#
#   docker build -t solouder-linux -f scripts/docker/linux.Dockerfile scripts/docker
FROM rust:1.98
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake libasound2-dev pulseaudio alsa-utils libasound2-plugins \
    && rm -rf /var/lib/apt/lists/*
RUN rustup component add rustfmt clippy
