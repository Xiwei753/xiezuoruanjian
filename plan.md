1.  **Update Android `SettingsActivity.kt` to route sync logic based on BackendType properly or default to `BackendType.GithubApi`**
    - The requirement states: "Android 默认使用 GitHub API 后端，或者在 Android 上将'立即同步'路由到 GitHub API 后端。" and "Linux 可以继续默认 Git/libgit2。"
    - In `SettingsActivity.kt`, I will ensure that the default backend type is `GithubApi`.
2.  **Enhance `GitHubApiBackend` in `sync_service.rs` to support `sync` operations**
    - Implement `GitHubApiBackend::sync()` using `reqwest` and `rustls-tls` to fetch remote tree and upload local files using GitHub REST API.
    - Since full 2-way sync with local working directory state is hard over plain REST API (need to manage local Git-like state without `libgit2` or just do simple upload), the instructions say: "如果完整 GitHub API 双向同步工作量过大，先实现安全的 Android 单向首同步/上传同步，但 UI 必须明确显示当前能力，不得假装完整双向同步已完成。"
    - So I will implement a unidirectional "upload" sync. It will:
        - Check if branch exists. If not, create it from `main` or default branch of the repo. (Or create an empty commit).
        - Get the current commit SHA of the branch.
        - Get the current tree SHA.
        - For each file in the sync whitelist (from `SyncService::build_sync_plan_from_workspace`), upload it as a blob.
        - Create a new tree with the new blobs.
        - Create a new commit.
        - Update the branch reference to the new commit.
    - I'll make sure not to upload `sync_secrets.local.json` and other ignored directories (already handled by `build_sync_plan_from_workspace`).
3.  **Enhance Git / libgit2 diagnostics to return specific TLS certificate error details**
    - Modify `perform_sync_diagnostics` (or `Git2Backend` diagnosis part) to catch `libgit2` certificate errors and set user messages explicitly.
    - If `reqwest` succeeds but `libgit2` fails with a certificate error, provide the required explicit message: `Android native libgit2 TLS certificate validation failed; GitHub API fallback is available`.
4.  **Complete pre commit steps**
    - Call `pre_commit_instructions` tool to get the required checks and verify my changes.
5.  **Submit the change.**
    - Submit the changes once everything passes.
