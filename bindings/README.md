# Bindings

This directory contains the integration and binding definitions allowing the native clients (`apps/android`, `apps/desktop`) to communicate with the shared Rust core (`core/writer_core`).

## Principles
1. **Core is the source of truth.** Native clients must NOT implement workspace formats, saving rules, or project management themselves.
2. **Core API / UniFFI.** All interactions are defined in `core/writer_core/src/api/` and UniFFI bindings are declared in `core/writer_core/src/api.udl`.
3. **Current State:** 
   - Android utilizes UniFFI to compile native libraries and generate bindings directly inside [apps/android](file:///home/xiwei/xiezuoruanjian/apps/android).
   - Desktop (Linux) integrates via QML-Rust native bindings configured within [apps/desktop](file:///home/xiwei/xiezuoruanjian/apps/desktop).

## Directories
- **[shared/](file:///home/xiwei/xiezuoruanjian/bindings/shared/)**: A placeholder directory reserved for future shared cross-platform binding utilities.

## Future Goal
The ultimate goal is for the UI layers of Android and Linux to rely exclusively on the typed APIs exposed by the Core, passing simple business IDs and root paths down to the Rust facade. The UI layers must remain completely unaware of the underlying file paths, JSON structures, or other storage mechanics.
