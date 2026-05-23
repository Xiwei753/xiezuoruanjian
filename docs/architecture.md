# Architecture

This repository uses a single-repository, multi-client, shared-core architecture.

> **Note:** For specific technical constraints and architecture route guidelines, please see the [Technical Route & Architecture Constraints](TECHNICAL_ROUTE.md) and the [Cross-Platform Capability Contract & Core-First Architecture Constraints](CROSS_PLATFORM_CAPABILITY_CONTRACT.md). These root documents govern the overall route and cross-platform capability alignment, while directory-level documents govern specific implementation boundaries.

## Core-First & Capability Contract

> **Capability Alignment Status:** Please check the [Cross-Platform Capability Matrix](CAPABILITY_MATRIX.md) to understand the current alignment status between Rust Core, Android JNI, and Linux backend, and the roadmap for refactoring bifurcated logic back into the core.

To prevent platform duplication and state bifurcation between different clients (such as Android and Linux), the repository enforces a **Core-First Architecture**:
- **Single Source of Truth**: `core/writer_core` is the sole owner of business logic, state mutations, and validation rules.
- **Pure Adapters**: The Android JNI layer and Linux Qt/QML backend act strictly as thin adapters translating Core data envelopes to UI views. They are forbidden from implementing separate business rules or modifying workspace files directly.
- **Contract Enforcement**: All shared features (Workspace, Project, Volume, Chapter, Settings, Sync, MindMap, Editor Model, AI) must implement the standard capability API contract defined in [CROSS_PLATFORM_CAPABILITY_CONTRACT.md](CROSS_PLATFORM_CAPABILITY_CONTRACT.md).

## Project Structure
- `core/writer_core`: The shared core library written in Rust. It handles platform-independent logic, document formatting, settings, and synchronization rules. UI, animations, input methods, and window logic are strictly excluded. (See [Rust Core Technical Route](../core/writer_core/TECHNICAL_ROUTE.md) for detailed boundaries).
- `apps/android`: A native Kotlin Android client targeting low power usage, stable IME, and consistent keyboard interactions. (See [Android Technical Route](../apps/android/TECHNICAL_ROUTE.md) for detailed boundaries).
- `apps/linux`: A native Linux client, targeting Qt/CMake for optimal integration with X11/Wayland/fcitx5. (See [Linux Technical Route](../apps/linux/TECHNICAL_ROUTE.md) for detailed boundaries).
- `bindings`: Code to interface between the Rust core and the native clients (Android and Linux).

All clients share the exact same workspace format and sync rules. The structure of the workspace is the definitive single source of truth for the document format.
