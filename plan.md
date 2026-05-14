1.  **Refactor Error Handling in `perform_sync`**:
    *   Change `best_path.unwrap()` to use `match` and log/skip on error.
    *   Fix `is_worktree_empty_or_git_only` to propagate errors instead of `unwrap_or(false)`.
    *   Ensure `save_sync_state` and `repo.cleanup_state` failures aren't ignored and update `SyncResult.error` and `SyncResult.user_message` appropriately.

2.  **Explicit First Sync Strategy (`FirstSyncMode`)**:
    *   Add enum `FirstSyncMode`: `CloneIntoEmptyWorkspace`, `InitExistingWorkspace`, `AlreadyGitRepo`, `BlockedNonEmptyRemote`, `UnrelatedHistories`.
    *   Add `first_sync_mode` and `user_message` to `SyncResult` to describe the result of first sync or sync operation for UI integration.
    *   Detect related/unrelated histories properly. Return error state without auto-allow unrelated histories.

3.  **Refine clone / init Behavior**:
    *   If the workspace is completely empty, it should be cloned (`CloneIntoEmptyWorkspace`).
    *   If the workspace has existing files but no `.git`, attempt to initialize (`InitExistingWorkspace`). Ensure it doesn't run `clone` in a non-empty directory.

4.  **Strengthen Remote Branch Handling**:
    *   Fallback `config.branch` to `"main"` if empty.
    *   If remote branch does not exist during push/pull, return explicit error instead of generic git2 error. Create remote branch on push if it doesn't exist.

5.  **Strengthen Conflict Handling**:
    *   Ensure "unknown" paths never appear in conflicts. Record errors for missing conflict paths.
    *   Only generate conflict data for whitelisted files. Do not generate `.conflict.*` files for blacklisted paths.

6.  **Fix Token/Secrets Isolation**:
    *   Ensure `sync_config.json`, `sync_state.json`, and upload lists don't have token data.
    *   Ensure `app-meta/sync/sync_secrets.local.json` and `.tmp` are strictly blacklisted. Verify via test explicitly checking token strings.

7.  **Add/Update Tests**:
    *   Update dry-run and config tests.
    *   Add tests for empty vs non-empty workspaces, clone vs init.
    *   Add test for commit before pull.
    *   Add test for non-existing remote branch error.
    *   Add test for unrelated histories failure.
    *   Add test for missing paths in conflict JSON.
    *   Add test for secrets blacklist and lack of leak.

8.  **Run formatting, linting, and tests**: `cargo fmt --all --check`, `cargo check --workspace`, `cargo test --workspace`, `tools/check_all.sh` (as mentioned in the prompt).
