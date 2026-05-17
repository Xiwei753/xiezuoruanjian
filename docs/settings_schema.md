# Settings Schema

This document outlines the JSON schema for application settings.

### `settings.local.json`
Stores device-specific configurations that should NOT be synchronized across devices.
Includes:
- `themeMode` (String, e.g. "system", "dark", "light")
- `locale` (String)
- `editorFontSize` (Float)
- `editorLineSpacingMultiplier` (Float)
- `autoSaveEnabled` (Boolean)
- `autoSaveDelayMs` (Integer)
- `windowWidth`, `windowHeight` (Float)

### `settings.sync.json`
Stores user preferences that SHOULD be synchronized across all devices.
Includes:
- AI API keys (saved in plaintext, explicitly accepted by user design)
- Editor preferences (`fontSize`, `themeMode`, indent styles)

### `sync_config.json`
Stores sync configuration. This defines which files/directories should be synced.
Includes:
- `remote_url` (String)
- `branch` (String)
- `auto_sync` (Boolean)
- `proxy` (Object)

### `sync_secrets.local.json`
Stores sensitive information like GitHub tokens, SSH private keys. This is saved locally and should NEVER be synchronized.
