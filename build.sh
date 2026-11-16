#!/usr/bin/env bash
# Windows git-bash 에서 빌드 → target/release/tempo.exe
# Rust x86_64-pc-windows-gnu 툴체인은 windows-link 의 raw-dylib 때문에 dlltool 이 필요하다.
#   예: w64devkit 의 dlltool.exe · as.exe 두 개만 따로 폴더에 복사해 PATH 에 추가 (gcc 까지 PATH 에 넣으면 링크가 깨짐)
set -e
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$HOME/tools/dlltool:$PATH"
# 실행 파일에 빌드 PC 경로(사용자 이름)가 박히지 않게 — 오류 메시지용 소스 경로를 짧게 바꿈 (뒤에 적은 규칙이 우선)
export RUSTFLAGS="--remap-path-prefix=$(cygpath -w "$HOME")=~ --remap-path-prefix=$(cygpath -w "$PWD")=tempo"
cargo test --release
cargo build --release
ls -la target/release/tempo.exe
