# Sujian Writer

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL%203.0-blue.svg)](LICENSE)
[![Linux Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/linux_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/linux_build.yml)
[![Android Build](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/android_debug_build.yml/badge.svg)](https://github.com/Xiwei753/xiezuoruanjian/actions/workflows/android_debug_build.yml)
<!-- Screenshot placeholder: replace with actual app screenshots -->
<!--
![Sujian Writer - Writing View](docs/screenshots/writing.png)
![Sujian Writer - StarMap](docs/screenshots/starmap.png)
-->

A cross-platform writing tool for novel creators, built with a Rust core + native client architecture to ensure data safety and writing experience.

## Features

- Plain text writing — your data always belongs to you
- Project / Volume / Chapter three-level organization
- Auto-save for peace of mind
- One-click formatting
- StarMap character relationship visualization
- AI writing assistant
- Cloud sync (Beta)
- Cross-platform: Android / Linux / HarmonyOS WIP; Windows will move to a separate native-client route later

> **Regarding Apple platforms**: There are currently no plans to support macOS / iOS. We'll consider it once we've saved up enough from our day jobs to cover the Apple Developer Program signing fee (¥688/year).

## Architecture

```
core/writer_core/     Rust core library (sole business logic layer)
apps/android/         Kotlin Android client
apps/Linux_qt/         Rust + Qt6/QML Linux client
apps/windows/         WinUI 3 / Windows App SDK native client
apps/harmony/         ArkTS HarmonyOS NEXT client
bindings/             Cross-platform binding code
```

- `core/writer_core`: The **sole** business logic core library written in Rust. Handles all file I/O, project management, sync, formatting, and settings rules. Strictly excludes UI logic.
- `apps/android`: Native Kotlin Android client. Main business entry point is `AppServiceBridge + UniFFI`. `BridgeProvider` only exposes domain Bridges to Repository/ViewModel/UI for platform adaptation.
- `apps/Linux_qt`: Native Rust + Qt6/QML Linux client. UI calls Rust Core through the QObject backend adapter layer. Implementing workspace, save, or sync rules in QML is not allowed. Current priorities are Linux IME, rendering, animation, AppImage, log export, and runtime profile stability.
- `apps/windows`: Windows native client route. The official route is WinUI 3 / Windows App SDK app shell + custom `SujianEditor` + DirectWrite/Direct2D + Windows IME + Rust `writer_core`. It does not reuse `apps/Linux_qt` Qt/QML. `RichEditBox` / `TextBox` cannot be the official writing area.
- `apps/harmony`: Native ArkTS HarmonyOS NEXT client. Calls Rust Core FFI through the NAPI C++ bridge layer. The ArkTS side is decoupled via the `IWriterCoreBridge` interface.
- `bindings`: Code for connecting the Rust core with native clients.

> Note: The previous Flutter client has been completely removed due to architectural conflicts.

For details, see the [Technical Route & Architecture Constraints](docs/TECHNICAL_ROUTE.md).

## Core Principles

- **Single Business Layer**: The Rust core (`core/writer_core`) is the sole entry point for file and data logic. Clients are not allowed to assemble paths or handle Workspace rules on their own.
- **Single Source of Truth**: `docs/workspace_format.md` defines the workspace format. It is maintained and operated by Rust Core.
- **Thin Clients**: Clients are only responsible for UI rendering, navigation, IME interaction, and theming. They should not contain any persistence or data logic.

## Development

### Tools

- `tools/build_core.sh`: Build the Rust core library.
- `scripts/generate_icons.py`: Generate app icon assets for each platform from `assets/brand/icon/source`.

### Rust Core

```bash
cd core/writer_core
cargo fmt
cargo check
cargo test
```

### Android Client

```bash
./tools/build_android.sh
```

**Supported targets**: Only `arm64-v8a` builds are officially supported. `x86_64` Android devices or emulators are not supported. To add `x86_64` support, open-source users can modify `tools/build_android.sh` and `apps/android/app/build.gradle.kts` to add the corresponding ABI.

### Linux Client

The Linux client uses Qt6 exclusively. CI, `apps/Linux_qt/build.rs`, and the directory README all use Qt6 as the sole build pipeline; do not mix in Qt5 QML/plugin paths.

```bash
cargo run -p sujian-linux-qt
```

If you encounter rendering issues (such as double UI or black screen misalignment under Wayland), try running with basic render loop and enabling debug logs:

```bash
QSG_INFO=1 QSG_RENDER_LOOP=basic cargo run -p sujian-linux-qt
```

### Windows Client

See `apps/windows/README.md`. Windows client is fixed in `apps/windows`: first verify the custom editor MVP (plain text display, caret positioning, input, delete, line breaks, arrow keys, scrolling, Microsoft Pinyin composition/commit, candidate window anchor, and opening/saving chapters via `writer_core`), then complete full pages.

### HarmonyOS Client

Open the `apps/harmony/` directory in DevEco Studio. You need to build the prebuilt Rust FFI library first:

```bash
./tools/build_harmony.sh
```

Requirements:
- Add `aarch64-unknown-linux-ohos` target to your Rust toolchain
- Set the `OHOS_NDK_HOME` environment variable

### Manual Testing Steps

- Create a new project.
- Enter the project and confirm that "Volume 1" appears automatically.
- Click to create a new chapter and type some content.
- Confirm auto-save works.
- Exit and re-enter the chapter, confirm the content is still there.

## Documentation

- `docs/TECHNICAL_ROUTE.md`: Global technical route and architecture constraints
- `docs/workspace_format.md`: Single source of truth for document structure on disk
- `docs/settings_schema.md`: Local and syncable settings definitions
- `docs/sync_rules.md`: Sync rules
- `docs/starmap_semantics.md`: StarMap semantic foundation (independent objects and reference safety)
- `docs/starmap_canvas_model.md`: StarMap canvas model contract
- `docs/starmap_implementation_route.md`: StarMap implementation route
- `docs/editor_engine_route.md`: Custom editor engine and unified event layer route
