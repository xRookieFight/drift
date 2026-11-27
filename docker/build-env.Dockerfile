# Build environment for Drift: Fedora plus everything Servo needs to compile.
# Package list mirrors servo/python/servo/platform/linux_packages/dnf/dnf_base.txt,
# keep them in sync when bumping Servo.
FROM registry.fedoraproject.org/fedora:44

LABEL org.opencontainers.image.source="https://github.com/xRookieFight/drift"
LABEL org.opencontainers.image.description="Build environment for the Drift browser (Servo deps + Rust)"
LABEL org.opencontainers.image.licenses="MPL-2.0"

RUN dnf install -y --setopt=install_weak_deps=False \
      libtool gcc-c++ libXi-devel freetype-devel libunwind-devel \
      mesa-libGL-devel mesa-libEGL-devel glib2-devel libX11-devel libXrandr-devel \
      gperf fontconfig-devel cabextract ttmkfdir expat-devel rpm-build cmake \
      libXcursor-devel libXmu-devel dbus-devel ncurses-devel harfbuzz-devel \
      ccache clang clang-libs llvm python3-devel \
      gstreamer1-devel gstreamer1-plugins-base-devel gstreamer1-plugins-good \
      gstreamer1-plugins-bad-free-devel gstreamer1-plugins-ugly-free \
      libjpeg-turbo-devel zlib-ng libjpeg-turbo vulkan-loader \
      libxkbcommon libxkbcommon-devel libxkbcommon-x11 wayland-devel \
      git curl tar xz file findutils which zstd \
    && dnf clean all

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal \
      --default-toolchain stable --component rustfmt,clippy \
    && chmod -R a+w "$RUSTUP_HOME" "$CARGO_HOME" \
    && rustc --version

WORKDIR /work
