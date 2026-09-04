# Codebase Structure — 素笺 (Sujian) Writer

## Tech Stack

- **Language**: Rust (edition 2021)
- **Workspace**: Cargo workspace，根 `Cargo.toml` 统一管理多 crate
- **Core crate**: `writer_core` (rlib)，业务核心，不依赖平台 crate
- **Platform API crate**: `writer_platform_api`，Core 所需的平台能力契约
- **UniFFI crate**: `writer_uniffi`，稳定的 UniFFI 导出门面
- **Platform Rust**: `platform/rust/{linux,android,harmony}`，各平台 Rust 适配
- **Apps**: `apps/Linux_qt`（Linux Qt 客户端）、`apps/android`（Android Kotlin/Compose）
- **Key deps**: git2 (vendored libgit2) 0.20.4、serde/serde_json、uuid、chrono、tempfile、rayon、crop、aho-corasick、uniffi 0.28

## Directory Structure (核心)

```
core/writer_core/src/
├── api/                    # 稳定 DTO、错误、服务边界（WriterCoreApi）
│   ├── service/            # API 聚合服务实现（mod/project_ops/starmap_ops/...）
│   ├── chapter_api.rs      # 章节 CRUD API
│   ├── sync_api.rs         # 全量同步 API
│   ├── settings_api.rs     # 设置 API
│   └── bootstrap.rs        # 启动/恢复
├── facade/                 # Core 业务入口（WriterCore）
│   ├── starmap_ops.rs      # StarMap facade ops
│   ├── sync_ops.rs         # 同步 facade ops（commit_full_sync）
│   └── ...
├── storage/                # 原子写入、Git layout、workspace_git、workspace_paths
│   ├── workspace_git/      # 本地 Git 版本历史（history/model/repo/recovery/rollback）
│   ├── workspace_paths.rs  # 路径分类事实来源
│   └── git_repo_layout/    # GitRepoLayout
├── starmap/                # 星图业务数据（mod.rs 直接持久化函数）
├── sync/                   # 同步协议、状态机、staging、commit_helpers
│   └── commit_helpers.rs   # staging commit + committed_paths 收集
├── chapter.rs / project.rs # 章节/作品持久化
└── delete_guard.rs         # 安全删除边界
```

## Build / Test / Lint Commands

```bash
# 编译核心
cargo build -p writer_core

# 核心测试
cargo test -p writer_core

# AI 专项测试
cargo test -p writer_core --features ai --test ai_feature

# 复现测试（Issue #645）
cargo test -p writer_core --test issue_645_comment_5504296097_repro

# Clippy（严格，-D warnings）
cargo clippy --package writer_core --all-targets --all-features -- -D warnings

# 格式检查
cargo fmt --all --check
cargo fmt --all   # 自动格式化

# Rust 安全守卫
python3 tools/check_rust_safety_patterns.py .
python3 tools/test_check_rust_safety_patterns.py
```

## Key Modules (本次重构相关)

- `storage/workspace_git/history.rs` — `record_workspace_changes`（待拆分为 paths / all 两个入口）
- `storage/workspace_paths.rs` — `is_workspace_history_path`（待拆分 WorkspacePathClass）
- `storage/workspace_git/model.rs` — `WorkspaceCommitResult`
- `api/service/mod.rs` — `WriterCoreApi::record_workspace_history`（待拆分对应）
- `api/chapter_api.rs` — 章节 API（create_chapter 漏 chapter.md）
- `api/service/project_ops.rs` — 作品/卷 API（delete_project/delete_volume 漏子文件）
- `api/service/starmap_ops.rs` — StarMap API（直接持久化无 history）
- `api/sync_api.rs` — 同步 API（save_sync_config/save_app_sync_state 误进 history）
- `starmap/mod.rs` — StarMap 直接持久化函数（待返回 WorkspaceChangeSet）
- `sync/commit_helpers.rs` — committed_paths 收集（engine_state_actions 不应进 history）

## Conventions

- 不建 `*_v2` / `legacy_*` / `fallback_*` 或并行状态机；优先修改已有模块
- 外部输入、锁、磁盘错误显式返回，不用 `unwrap`/`expect` 处理服务路径
- 不手写 `unsafe impl Send/Sync`；新 `unsafe` 只允许必要 FFI 并写 `SAFETY:` 说明
- 不用宽范围 `allow` 掩盖 warning
- 删除作品/卷/章节/目录沿用 `delete_guard`，不绕过直接拼路径递归删除
- Core 不依赖 Android/Qt/Compose 等平台类型
