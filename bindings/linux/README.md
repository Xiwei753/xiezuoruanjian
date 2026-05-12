# Linux Bindings (FFI)

This directory contains the C-compatible FFI bindings to connect `apps/linux` with `core/writer_core`.

## Architecture Note
The Linux client will strictly use these FFI bindings to interact with `core/writer_core/src/facade.rs`.
The UI layer is forbidden from accessing the workspace format directly.

**Important**: The FFI layer must strictly expose the high-level `facade.rs` API. It must NOT leak the workspace file structure, file paths, or serialization details to the Linux C++ layer. The Linux app only provides the workspace root path and business IDs.
