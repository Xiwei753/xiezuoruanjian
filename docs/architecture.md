# Architecture

This repository uses a single-repository, multi-client, shared-core architecture.

- `core/writer_core`: The shared core library written in Rust. It handles platform-independent logic, document formatting, settings, and synchronization rules. UI, animations, input methods, and window logic are strictly excluded.
- `apps/flutter_legacy`: The existing Flutter client, kept as a legacy prototype and reference.
- `apps/android_native`: A native Kotlin Android client targeting low power usage, stable IME, and consistent keyboard interactions.
- `apps/linux_native`: A native Linux client, targeting Qt/CMake for optimal integration with X11/Wayland/fcitx5.
- `bindings`: Code to interface between the Rust core and the native clients (Android and Linux).

All clients share the exact same workspace format and sync rules. The structure of the workspace is the definitive single source of truth for the document format.
