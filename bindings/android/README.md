# Android Bindings (JNI)

This directory contains the JNI bindings to connect `apps/android` with `core/writer_core`.

## Architecture Note
Currently, Android uses a `TemporaryWorkspaceBridge` and `TemporarySettingsBridge`.
The next stage of development will replace these bridges with JNI calls that directly interact with `core/writer_core/src/facade.rs`.

**Important**: The JNI layer must strictly expose the high-level `facade.rs` API. It must NOT leak the workspace file structure, file paths, or serialization details to the Android layer. Android only provides the workspace root path and business IDs (e.g., project ID, chapter ID).
