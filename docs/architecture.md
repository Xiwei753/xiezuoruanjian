# Architecture

This repository uses a single-repository, multi-client, shared-core architecture.

> **Note:** For specific technical constraints and architecture route guidelines (especially for new features like the Android Mind Map), please see [Technical Route & Architecture Constraints](TECHNICAL_ROUTE.md). This root document governs the overall route, while directory-level documents govern specific implementation boundaries.

- `core/writer_core`: The shared core library written in Rust. It handles platform-independent logic, document formatting, settings, and synchronization rules. UI, animations, input methods, and window logic are strictly excluded. (See [Rust Core Technical Route](../core/writer_core/TECHNICAL_ROUTE.md) for detailed boundaries).
- `apps/android`: A native Kotlin Android client targeting low power usage, stable IME, and consistent keyboard interactions. (See [Android Technical Route](../apps/android/TECHNICAL_ROUTE.md) for detailed boundaries).
- `apps/linux`: A native Linux client, targeting Qt/CMake for optimal integration with X11/Wayland/fcitx5. (See [Linux Technical Route](../apps/linux/TECHNICAL_ROUTE.md) for detailed boundaries).
- `bindings`: Code to interface between the Rust core and the native clients (Android and Linux).

All clients share the exact same workspace format and sync rules. The structure of the workspace is the definitive single source of truth for the document format.
