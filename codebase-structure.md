# Codebase Structure — 素笺（Sujian）写作工具

## 项目概述

跨平台写作工具，业务核心用 Rust 实现，平台端（Android Kotlin/Compose、Linux Qt）负责 UI 与系统能力。Core 是作品、卷、章节、正文、设置、同步、统计、星图和数据格式的唯一事实来源。

## 技术栈

- **语言**: Rust（workspace，根 `Cargo.toml`）、Kotlin（Android）、C++/Qt（Linux）
- **核心 crate**: `writer_core`（业务核心）、`writer_platform_api`（平台能力契约）、`writer_uniffi`（UniFFI 导出）
- **关键依赖**: git2 0.20.4（vendored libgit2）、serde、serde_json、tempfile、uuid、chrono
- **构建**: Cargo

## 目录结构

```
core/
  writer_core/          # 业务核心（本 issue 目标）
    src/
      storage/
        git_repo_layout.rs            # Git 仓库布局迁移 + 恢复状态机（缺陷1所在）
        project_delete_transaction.rs # 项目删除事务 + 恢复（缺陷2所在）
        transaction.rs                # 通用 durable transaction
        git_runtime.rs                # libgit2 运行时初始化
      project.rs / volume.rs / chapter.rs  # 作品结构
      editor/  sync/  settings/  starmap/  writing_stats/
    tests/            # 集成测试（复现测试放这里）
  writer_platform_api/  # 平台能力契约
  writer_uniffi/        # UniFFI 导出
apps/android/  apps/Linux_qt/   # 平台客户端
docs/                   # 长期架构与数据格式契约
```

## 构建与测试命令

- 构建：`cargo build -p writer_core`
- 全量测试：`cargo test -p writer_core`
- 单个集成测试：`cargo test -p writer_core --test <name>`
- Clippy：`cargo clippy --package writer_core --all-targets --all-features -- -D warnings`
- 格式：`cargo fmt --all --check`
- Rust 安全守卫：`python3 tools/check_rust_safety_patterns.py .`

## 关键模块（本 issue 相关）

- `storage::git_repo_layout`
  - `GitRepoLayout`（pub struct）：worktree_root + git_dir
  - `ensure_project_repo_with_layout`（pub fn）：入口，先调 `resume_layout_migration` 再 init/迁移
  - `resume_layout_migration`（私有）：恢复 pending 迁移，按 MigrationPhase 状态机推进
  - `MigrationPhase`：Prepared → SourceClaimed → TargetPrepared → TargetInstalled → SourceCleaned
  - `LayoutMigrationJournal`：owner/worktree_canonical/original_source/claimed_source/target_tmp/target_git_dir/phase
  - journal 路径：`<target_git_dir.parent()>/.layout-migrations/<owner>.json`
- `storage::project_delete_transaction`
  - `recover_pending_delete_transactions`（pub fn）：启动恢复入口
  - `recover_single_journal`（私有）：按 ProjectDeletePhase 恢复单个 journal
  - `ProjectDeletePhase`：Prepared → WorktreeMoved → GitMoved → Completed
  - `ProjectDeleteJournal`（pub struct，字段 pub）
  - `cleanup_journal`（pub fn）：正确的 durable cleanup 范式（remove_file + sync_dir(parent)）
  - journal 目录：`<app_data_root>/app-meta/delete-journals/`
- `storage::sync_dir`（pub fn）：fsync 目录，durable cleanup 的基础

## 安全边界

- 不手写 `unsafe impl Send/Sync`；`unsafe` 仅限 FFI 边界并附 `SAFETY:`
- 外部输入/锁/磁盘/网络错误显式返回，不用 `unwrap`/`expect` 处理服务路径
- 不用宽范围 `allow` 掩盖 warning
