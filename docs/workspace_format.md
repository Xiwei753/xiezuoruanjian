# Workspace Format

The workspace format defines the exact directory structure and file formats for storing writer documents, metadata, settings, and caches. It serves as the single source of truth for the shared Rust core and all native clients.

```
workspace/
├─ workspace_manifest.json       # Basic workspace info
├─ app-meta/                     # Global metadata and settings
│  ├─ settings/
│  │  ├─ settings.local.json     # Device-specific settings (NOT synced)
│  │  └─ settings.sync.json      # Cross-device settings (synced)
│  └─ logs/                      # App logs
├─ projects/
│  └─ <project_id>/              # Project directory
│     ├─ project.json            # Project metadata
│     ├─ volumes/
│     │  └─ <volume_id>/         # Volume directory
│     │     ├─ volume.json       # Volume metadata
│     │     └─ chapters/
│     │        └─ <chapter_id>/  # Chapter directory
│     │           ├─ chapter.md  # Main text
│     │           └─ chapter.meta.json # Chapter metadata
│     └─ characters/             # Character cards (if applicable)
├─ backups/                      # Backup files
├─ trash/                        # Deleted files
└─ sqlite_cache/                 # Rebuildable cache (NOT the source of truth)
```
