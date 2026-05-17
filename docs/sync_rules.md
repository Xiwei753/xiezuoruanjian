# Sync Rules

This document outlines the synchronization and conflict resolution rules for the shared core.
- All changes are synchronized to a private Git repository.
- Conflicts are stored as separate conflict files and never automatically overwrite local data.

## Continuous Integration (CI)
- Default GitHub Actions workflows are strictly for building the Android debug APK.
- Linux and desktop builds should be executed manually on the local machine by the user.

## Synchronization Rules
- Data sync follows strict whitelist/blacklist configurations defined in the workspace.
- `app-meta/settings/settings.local.json` and `app-meta/sync/sync_secrets.local.json` are blacklisted and only kept locally.
