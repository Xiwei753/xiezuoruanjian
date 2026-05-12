#!/bin/bash
export RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=lld"
cargo build --manifest-path bindings/android/Cargo.toml --target x86_64-linux-android
