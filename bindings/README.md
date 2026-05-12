# Bindings

This directory contains the integration code that allows the native clients (`apps/android`, `apps/linux`) to communicate with the shared Rust core (`core/writer_core`).

## Principles
1. **Core is the source of truth.** Native clients must NOT implement workspace formats, saving rules, or project management themselves.
2. **Strict Facade.** All interaction must go through `core/writer_core/src/facade.rs`.
3. **Current State:** The JNI and FFI implementation is currently a stub. Android is using a temporary Kotlin bridge until this layer is fully functional.

## Directories
- `android/`: JNI code and Kotlin/JNA wrappers.
- `linux/`: C/C++ FFI wrappers for Qt.
