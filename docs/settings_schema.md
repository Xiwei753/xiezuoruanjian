# Settings Schema

This document outlines the JSON schema for application settings.

### `settings.local.json`
Stores device-specific configurations that should NOT be synchronized across devices.
Includes:
- Window bounds, theme mode (if device-specific)
- Local workspace path

### `settings.sync.json`
Stores user preferences that SHOULD be synchronized across all devices.
Includes:
- AI API keys (saved in plaintext, explicitly accepted by user design)
- Editor preferences (font size, indent styles)
- Sync settings (Git tokens, repo URL)
