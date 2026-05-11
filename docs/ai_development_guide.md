# AI Development Guide

When contributing to this repository using AI, strictly adhere to the following rules:

1. **Workspace Format is Sacred**: Do not modify the `workspace_format.md` or change how files are stored on disk merely to accommodate UI needs. The workspace format is the single source of truth.
2. **Docs and Tests First**: When modifying the Rust `writer_core`, you MUST update `docs/core_api.md` and write corresponding `cargo test`s.
3. **Keep Core Pure**: Do not inject platform-specific UI logic, animation loops, window management, or input method (IME) handling into `writer_core`. The core is strictly for data, logic, and file I/O.
4. **Data Privacy**: Never output user manuscript content, API keys, or security tokens into application logs.
5. **Settings Schema**: Any new application setting must be documented in `docs/settings_schema.md` and handled appropriately as local or syncable.
