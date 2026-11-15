#!/bin/bash
# Compila o ilmarfy-ffi para Android (arm64) e gera as classes Kotlin do UniFFI.
# Chamado pelo Gradle antes do build; também pode rodar sozinho.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
APP="$ROOT/android/app"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$(ls -d "$HOME/Library/Android/sdk/ndk/"* | sort -V | tail -1)}"
# Usa o Rust do rustup (o do Homebrew não tem os alvos do Android)
TOOLCHAIN_BIN="$(dirname "$(rustup which cargo)")"
export PATH="$TOOLCHAIN_BIN:$HOME/.cargo/bin:$PATH"
PROFILE="${RUST_PROFILE:-release}"

cargo ndk -t arm64-v8a -o "$APP/src/main/jniLibs" build -p ilmarfy-ffi --lib $([ "$PROFILE" = release ] && echo --release)

# As classes Kotlin saem dos metadados da própria biblioteca (compilada para o Mac só para isso)
cargo build -q -p ilmarfy-ffi --lib
cargo run -q -p ilmarfy-ffi --bin uniffi-bindgen -- generate \
    --library "$ROOT/target/debug/libilmarfy_ffi.dylib" \
    --language kotlin --no-format \
    --out-dir "$APP/build/generated/uniffi"
