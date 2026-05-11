# Sync Rules

This document outlines the synchronization and conflict resolution rules for the shared core.
(To be defined in detail as the remote sync implementation evolves).
- All changes are synchronized to a private Git repository.
- Conflicts are stored as separate conflict files and never automatically overwrite local data.
