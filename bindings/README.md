# Bindings

This directory contains the integration code that allows the native clients (`apps/android`, `apps/desktop`) to communicate with the shared Rust core (`core/writer_core`).

## Principles
1. **Core is the source of truth.** Native clients must NOT implement workspace formats, saving rules, or project management themselves.
2. **Strict Facade.** All interaction must go through `core/writer_core/src/facade.rs`.
3. **Current State:** The JNI and FFI implementation is currently a stub. Android is using a temporary Kotlin bridge until this layer is fully functional.

## Directories
- `android/`: JNI code and Kotlin/JNA wrappers. See [Android Bindings README](android/README.md) for details.
- `linux/`: C/C++ FFI wrappers for Qt. See [Linux Bindings README](linux/README.md) for details.

## Future Goal
The ultimate goal is for the UI layers of Android and Linux to rely exclusively on the APIs exposed by these bindings, which will pass simple business IDs and root paths down to the Rust facade. The UI layers must remain completely unaware of the underlying file paths, JSON structures, or other storage mechanics.
