#!/bin/bash
# Builds RyM (terminal) and RymPlayer-GTK on Ubuntu (WSL), from the Windows repo's commits.
set -e
export DEBIAN_FRONTEND=noninteractive
echo "== packages"
apt-get update -qq
apt-get install -y -qq --no-install-recommends \
  build-essential pkg-config cmake git curl ca-certificates file \
  libasound2-dev libssl-dev libgtk-3-dev libwebkit2gtk-4.1-dev libxdo-dev \
  libgtk-4-dev libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev \
  libfuse2t64 >/dev/null
echo "== rust"
if [ ! -x "$HOME/.cargo/bin/cargo" ]; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
fi
source "$HOME/.cargo/env"
rustc --version
echo "== source"
rm -rf ~/RyM
git clone -q /mnt/c/Users/Willy/RyM ~/RyM
cd ~/RyM
git log --oneline -1
echo "== build"
export CMAKE=cmake
cargo build --release --bin RyM 2>&1 | grep -E "^(error|warning: unused)|^  -->|Finished" || true
cargo build --release --no-default-features --features gtk-ui --bin RymPlayer-GTK 2>&1 | grep -E "^error|^  -->|Finished" || true
ls -la target/release/RyM target/release/RymPlayer-GTK 2>&1
echo "== done"
