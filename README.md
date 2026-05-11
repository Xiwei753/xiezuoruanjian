# Writer App (Multi-Client Architecture)

This repository contains the source code for a cross-platform writing application, transitioning to a single-repository, multi-client, shared-core architecture.

## Architecture

- `core/writer_core`: The shared core library written in Rust. It handles platform-independent logic, document formatting, settings, and synchronization rules. UI, animations, input methods, and window logic are strictly excluded.
- `apps/flutter_legacy`: The existing Flutter client, kept as a legacy prototype and reference.
- `apps/android_native`: A native Kotlin Android client (skeleton). Targets low power usage, stable IME, and consistent keyboard interactions.
- `apps/linux_native`: A native Linux client (skeleton), targeting Qt/CMake for optimal integration with X11/Wayland/fcitx5.
- `bindings`: Code to interface between the Rust core and the native clients.

See `docs/architecture.md` for more details.

## Core Principles

- **Single Source of Truth**: The `docs/workspace_format.md` defines the exact directory structure and file formats. All clients must adhere to this. UI layers cannot privately alter the format.
- **Platform Independence in Core**: The Rust core (`core/writer_core`) only handles file I/O, data logic, and platform-agnostic formatting.
- **Unified Sync Rules**: All clients share the same rules for synchronization and conflict resolution (`docs/sync_rules.md`).

## Development

### Tools

- `tools/validate_workspace.py`: Validates the structure of a workspace against the `docs/workspace_format.md` specification.
- `tools/check_all.sh`: Runs formatting, linting, tests, and fixture validation.
- `tools/build_core.sh`: Builds the Rust core library.
- `tools/build_android_native.sh`: Placeholder for building the Android client.
- `tools/build_linux_native.sh`: Builds the Linux client skeleton.
- `tools/build_flutter_legacy.sh`: Builds the legacy Flutter client.

### Rust Core

```bash
cd core/writer_core
cargo build
cargo test
```

### Flutter Legacy

To run the legacy Flutter application:

```bash
cd apps/flutter_legacy
flutter pub get
# The standard run command may still work for testing legacy logic
flutter run
```

*Note: The Flutter client is no longer the primary focus for new feature development. Its structure is preserved as a prototype reference.*

## Documentation

- `docs/architecture.md`: Overall architecture
- `docs/workspace_format.md`: Single source of truth for the document structure on disk.
- `docs/settings_schema.md`: Local and syncable settings definitions.
- `docs/core_api.md`: Rust core library API.
- `docs/sync_rules.md`: Synchronization rules.
- `docs/client_feature_matrix.md`: Matrix of client capabilities.
- `docs/ai_development_guide.md`: Important instructions for AI-assisted development.
