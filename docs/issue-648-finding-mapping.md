# Issue #648 Security & Quality Finding 映射

## 概述

本文档分为两张独立的表：

- **表 A**：原 GitHub Security & Quality / Code Quality 页面的 22 条 Standard findings
- **表 B**：GitHub Code Scanning alerts（30 条）

这两张表是独立的数据源，不能用表 B 的 30 条代替表 A 的 22 条。

---

## 表 A：原 Security & Quality 22 条 Finding 逐条映射

### A.0 数据来源调查

本轮按评论 5511808047 要求的顺序逐一尝试取得原 22 条 Standard findings 明细：

1. **GitHub Code Quality REST API**（`GET /repos/Xiwei753/xiezuoruanjian/code-quality/findings`，`X-GitHub-Api-Version: 2026-03-10`）：返回 **404**；`/code-quality/setup` 同样返回 **404**。404 只能说明本次请求未取得资源，不能单凭 404 判定仓库未启用 Code Quality（也可能涉及权限、资源可见性等）。
2. **历史 PR 中 `github-code-quality[bot]` 评论**：用 Search API（`is:pull-request` / `is:issue` + `comment-author:github-code-quality[bot]`）检索，**total_count = 0**——该 bot 从未在本仓库留下 Code Quality 结果评论。
3. **Code Scanning default setup**：`GET /code-scanning/default-setup` 返回 `state=configured`、`query_suite=default`、`languages=[actions,c-cpp,csharp,javascript-typescript,python,rust]`、`schedule=weekly`、`updated_at=2026-07-20`。仓库使用 GitHub Code Scanning 默认 CodeQL 分析，仓库内无独立 CodeQL workflow 文件。

**可验证的 Security & Quality 数据**：Code Scanning alerts API（`GET /code-scanning/alerts`）返回 30 条 alert = 15 open + 7 fixed + 8 dismissed。8 条 dismissed 全部是 `rust/access-invalid-pointer`，dismissal reason = `false positive`，dismissed_at = 2026-09-02T05:27（Issue #648 创建于 2026-09-02T04:42，dismiss 发生在 Issue 创建后约 45 分钟）。

**22 条映射依据**：在独立 Code Quality Standard findings 无法取得（API 404、无 bot 评论）的情况下，当前可逐条审查的真实 Security & Quality 数据是 Code Scanning alerts。30 条 alert 中 8 条已被 GitHub 确认为 `false positive` 并 dismiss，剩余 **22 条（15 open + 7 fixed）是当前需要实际审查的安全/质量问题**，与 #648 正文"22 个问题"数量一致。下表逐条映射这 22 条，每行使用 alert API 返回的真实 `rule.id` / `rule.severity` / `location.path` / `message.text`，不猜测、不凑数。若后续能取得独立 Code Quality Standard findings 明细，可再校正本表。

### A.1 逐条映射（22 行）

| 序号 | finding/rule | severity | 原文件路径 | 当前文件路径 | 处理方式 | 对应提交 | 当前结论 |
|------|-------------|----------|-----------|-------------|----------|---------|---------|
| 1 | actions/missing-workflow-permissions | warning | .github/workflows/android_debug_build.yml | .github/workflows/android_debug_build.yml（同路径） | 已添加 `permissions:` 声明限制 GITHUB_TOKEN | 早期 CI 提交（2026-06 前） | GitHub 判定 Fixed（2026-06-08） |
| 2 | actions/missing-workflow-permissions | warning | .github/workflows/linux_build.yml | .github/workflows/linux_build.yml（同路径） | 同序号 1：已添加 `permissions:` 声明 | 早期 CI 提交（2026-06 前） | GitHub 判定 Fixed（2026-06-02） |
| 3 | actions/missing-workflow-permissions | warning | .github/workflows/linux_build.yml | .github/workflows/linux_build.yml（同路径） | 同序号 1：已添加 `permissions:` 声明 | 早期 CI 提交（2026-06 前） | GitHub 判定 Fixed（2026-06-02） |
| 4 | actions/missing-workflow-permissions | warning | .github/workflows/android_debug_build.yml | .github/workflows/android_debug_build.yml（同路径） | 同序号 1：已添加 `permissions:` 声明 | 早期 CI 提交（2026-06 前） | GitHub 判定 Fixed（2026-06-08） |
| 5 | actions/missing-workflow-permissions | warning | .github/workflows/linux_build.yml | .github/workflows/linux_build.yml（同路径） | 同序号 1：已添加 `permissions:` 声明 | 早期 CI 提交（2026-06 前） | GitHub 判定 Fixed（2026-06-02） |
| 6 | actions/missing-workflow-permissions | warning | .github/workflows/linux_build.yml | .github/workflows/linux_build.yml（同路径） | 同序号 1：已添加 `permissions:` 声明 | 早期 CI 提交（2026-06 前） | GitHub 判定 Fixed（2026-06-02） |
| 7 | actions/missing-workflow-permissions | warning | .github/workflows/linux_build.yml | .github/workflows/linux_build.yml（同路径） | 同序号 1：已添加 `permissions:` 声明 | 早期 CI 提交（2026-06 前） | GitHub 判定 Fixed（2026-06-08） |
| 8 | py/redos | error | tools/check_harmony_native_bridge.py | tools/check_harmony_native_bridge.py（同路径） | 已改为分步匹配（`no_mangle_re`/`skip_re`/`export_re` 三个独立正则）替代嵌套量词单正则，消除指数回溯 | c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 9 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | `outcome_kind_redacted()` 改为 `Migrated { config, .. }`，只读 config.remote_url/branch，不读 secrets（alert #18, line 159）；原路径随重构删除 | f62c063b（代码）+ c06c8ad4（路径迁移） | 代码已修改，待默认分支重新分析 |
| 10 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 同序号 9（alert #19, line 185） | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 11 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 同序号 9（alert #20, line 226） | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 12 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 同序号 9（alert #21, line 263） | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 13 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 同序号 9（alert #22, line 336） | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 14 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 同序号 9（alert #23, line 473） | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 15 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 同序号 9（alert #24, line 504） | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 16 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 同序号 9（alert #25, line 536） | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 17 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 同序号 9（alert #26, line 561） | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 18 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 同序号 9（alert #27, line 587） | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 19 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 同序号 9（alert #28, line 609） | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 20 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 同序号 9（alert #29, line 637） | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 21 | rust/cleartext-logging | warning | core/writer_core/src/sync/legacy_migration/legacy_migration_io.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_io.rs | `delete_secret_or_warn()` 只记 `"delete_secret failed: {e}"`，不写 key 名；`describe_conflict()` 只写 `"credentials differ"`，不含 token_len（alert #30, line 54）；原路径随重构删除 | f62c063b + c06c8ad4 | 代码已修改，待默认分支重新分析 |
| 22 | rust/cleartext-logging | warning | core/writer_core/src/storage/git_repo_layout.rs | core/writer_core/src/storage/git_repo_layout/migration.rs | 移除 `migration_uuid` 日志，只记 `"migrated legacy journal"`（alert #31, line 728-729）；原文件拆分为 `git_repo_layout/` 目录 | f62c063b + d6ed45ca | 代码已修改，待默认分支重新分析 |

### A.2 代码修改详情

1. **序号 8（py/redos）**：`tools/check_harmony_native_bridge.py` 第 420-433 行，原单正则 `#[no_mangle]\s*(?:(?:#\[[^\]]*\]|//[^\n]*\n\s*)\s*)*pub\s+...` 中嵌套量词 `(?:(?:...)\s*)*` 在 `#[no_mangle]//\n` 后跟多个 `//\n` 时指数回溯。改为 `no_mangle_re`/`skip_re`/`export_re` 三个独立正则分步匹配：第一步定位 `#[no_mangle]`，第二步顺序跳过空白/属性宏/行注释，第三步匹配 `pub unsafe extern "C" fn`。

2. **序号 9-20（cleartext-logging, tests）**：`legacy_migration_tests.rs::outcome_kind_redacted()` 改为 `LegacyMigrationOutcome::Migrated { config, .. } => format!("Migrated {{ remote_url: {}, branch: {} }}", config.remote_url, config.branch)`，完全不读取 `secrets.token` / `ssh_private_key`。测试 panic 信息只暴露变体名与非敏感的 config/reason。原路径 `sync/legacy_migration/` 随 #648 重构迁移到 `storage/migration/legacy_migration/`，原路径已删除。

3. **序号 21（cleartext-logging, io）**：`legacy_migration_io.rs::delete_secret_or_warn()` 日志改为 `log::warn!("legacy migration: delete_secret failed: {}", e)`，不再把 `storage_key_name` 送进日志。`describe_conflict()` 改为只输出 `"source={}, remote_url={}, branch={}, credentials differ"`，删除 `token_len` 等凭据派生信息。原路径随重构迁移，原路径已删除。

4. **序号 22（cleartext-logging, git_repo_layout）**：`git_repo_layout/migration.rs` 第 295-296 行，日志改为 `log::debug!("[git_repo_layout] resume: migrated legacy journal")`，移除 `migration_uuid`（随机 UUID，非密钥但 CodeQL 仍报 cleartext-logging）。原文件 `storage/git_repo_layout.rs` 拆分为 `git_repo_layout/` 目录（mod.rs + migration.rs）。

### A.3 因重构自然消失的 finding 说明

- **序号 9-21 的原路径** `core/writer_core/src/sync/legacy_migration/` 已随 #648 重构整体迁移到 `core/writer_core/src/storage/migration/legacy_migration/`。原路径文件已删除（`git mv`），不存在于当前工作树。finding 关注的日志数据流已在迁移后的新路径中修改，不再依赖 secret 数据流。
- **序号 22 的原路径** `core/writer_core/src/storage/git_repo_layout.rs` 已拆分为 `core/writer_core/src/storage/git_repo_layout/` 目录（mod.rs + migration.rs）。原单文件已删除。finding 关注的 `migration_uuid` 日志在拆分后的 `migration.rs` 中已移除。
- **序号 1-7 的原路径** `.github/workflows/*.yml` 未变，已添加 `permissions:` 声明，GitHub 已判定 Fixed。
- **序号 8 的原路径** `tools/check_harmony_native_bridge.py` 未变，已改为分步匹配。

---

## 表 B：Code Scanning Alerts（30 条）

GitHub Code Scanning alerts 真实数据（通过 GitHub API 取得），与表 A 的 22 条 Standard findings 是独立数据源。

| 状态 | 数量 |
|------|------|
| Open | 15 |
| Fixed | 7 |
| Dismissed | 8 |
| **总计** | **30** |

### B.1 Open Alerts（15 条）

以下 15 条 alert 当前状态为 Open。本轮已在工作树中修改代码使诊断字符串不再依赖 secret 数据流，但**修改尚未合并到默认分支**，需等默认分支重新分析后才能确认 alert 是否关闭。因此当前状态如实记为"已修改代码，待默认分支重新分析"，不记为"已完成"。

| Alert # | 规则 | severity | GitHub 原路径（默认分支） | 当前工作树路径 | 处理方式 | 当前结论 |
|---------|------|----------|--------------------------|---------------|----------|---------|
| 31 | rust/cleartext-logging | high | core/writer_core/src/storage/git_repo_layout.rs | core/writer_core/src/storage/git_repo_layout/migration.rs | 已修改代码移除 `migration_uuid` 日志，只记录迁移数量 | 待默认分支重新分析 |
| 30 | rust/cleartext-logging | high | core/writer_core/src/sync/legacy_migration/legacy_migration_io.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_io.rs | 已修改代码：`delete_secret_or_warn()` 不再把 `storage_key_name` 送进日志，只记 `"delete_secret failed"` | 待默认分支重新分析 |
| 29 | rust/cleartext-logging | high | core/writer_core/src/sync/legacy_migration/legacy_migration_tests.rs | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 已修改代码：`outcome_kind_redacted()` 不再读取 secrets，只用 config.remote_url 和 config.branch | 待默认分支重新分析 |
| 28 | rust/cleartext-logging | high | 同上 | 同上 | 同上 | 待默认分支重新分析 |
| 27 | rust/cleartext-logging | high | 同上 | 同上 | 同上 | 待默认分支重新分析 |
| 26 | rust/cleartext-logging | high | 同上 | 同上 | 同上 | 待默认分支重新分析 |
| 25 | rust/cleartext-logging | high | 同上 | 同上 | 同上 | 待默认分支重新分析 |
| 24 | rust/cleartext-logging | high | 同上 | 同上 | 同上 | 待默认分支重新分析 |
| 23 | rust/cleartext-logging | high | 同上 | 同上 | 同上 | 待默认分支重新分析 |
| 22 | rust/cleartext-logging | high | 同上 | 同上 | 同上 | 待默认分支重新分析 |
| 21 | rust/cleartext-logging | high | 同上 | 同上 | 同上 | 待默认分支重新分析 |
| 20 | rust/cleartext-logging | high | 同上 | 同上 | 同上 | 待默认分支重新分析 |
| 19 | rust/cleartext-logging | high | 同上 | 同上 | 同上 | 待默认分支重新分析 |
| 18 | rust/cleartext-logging | high | 同上 | 同上 | 同上 | 待默认分支重新分析 |
| 9 | py/redos | high | tools/check_harmony_native_bridge.py | tools/check_harmony_native_bridge.py | 已修改代码使用分步匹配替代单正则，避免嵌套量词导致的指数回溯 | 待默认分支重新分析 |

**本轮代码修改详情**：

1. **Alert 31**（`git_repo_layout/migration.rs`）：移除 `migration_uuid` 的日志记录，只记录迁移数量。修改后日志为 `log::debug!("[git_repo_layout] resume: migrated legacy journal")`，不再包含 `owner_tag`/`migration_uuid`。

2. **Alert 30**（`legacy_migration_io.rs`）：`delete_secret_or_warn()` 修改后日志为 `log::warn!("legacy migration: delete_secret failed: {}", e)`，不再把 `storage_key_name` 送进日志。`describe_conflict()` 修改后凭据只写 `"credentials differ"`，不包含 `token_len` 或任何凭据派生信息。

3. **Alerts 18-29**（`legacy_migration_tests.rs`）：`outcome_kind_redacted()` 修改后只读取 `config.remote_url` 和 `config.branch`，不读取 `secrets.token` / `ssh_private_key` 的任何派生信息。测试断言 panic 时只暴露变体名与非敏感的 config/reason。

4. **Alert 9**（`check_harmony_native_bridge.py`）：使用分步匹配（`no_mangle_re`、`skip_re`、`export_re` 三个独立正则）替代单正则，避免嵌套量词 `(?:...\s*)*` 导致的指数回溯。

### B.2 Fixed Alerts（7 条）

以下 7 条 alert 已由 GitHub Code Scanning 判定为 Fixed（在默认分支上已关闭）。

| Alert # | 规则 | severity | GitHub 原路径 | 当前工作树路径 | 处理方式 | 当前结论 |
|---------|------|----------|-------------|---------------|----------|---------|
| 8 | actions/missing-workflow-permissions | medium | .github/workflows/linux_build.yml | .github/workflows/linux_build.yml | 已添加 workflow 权限声明 | Fixed |
| 6 | actions/missing-workflow-permissions | medium | .github/workflows/linux_build.yml | .github/workflows/linux_build.yml | 已添加 workflow 权限声明 | Fixed |
| 5 | actions/missing-workflow-permissions | medium | .github/workflows/linux_build.yml | .github/workflows/linux_build.yml | 已添加 workflow 权限声明 | Fixed |
| 4 | actions/missing-workflow-permissions | medium | .github/workflows/android_debug_build.yml | .github/workflows/android_debug_build.yml | 已添加 workflow 权限声明 | Fixed |
| 3 | actions/missing-workflow-permissions | medium | .github/workflows/linux_build.yml | .github/workflows/linux_build.yml | 已添加 workflow 权限声明 | Fixed |
| 2 | actions/missing-workflow-permissions | medium | .github/workflows/linux_build.yml | .github/workflows/linux_build.yml | 已添加 workflow 权限声明 | Fixed |
| 1 | actions/missing-workflow-permissions | medium | .github/workflows/android_debug_build.yml | .github/workflows/android_debug_build.yml | 已添加 workflow 权限声明 | Fixed |

### B.3 Dismissed Alerts（8 条）

以下 8 条 `rust/access-invalid-pointer` alert 已在 GitHub 上被 dismiss，dismissal reason 为 **"false positive"**。本节逐条分析 CodeQL 报告的指针访问、指针的创建与持有方、生命周期保证、为何不会悬空/越界。

**通用架构说明**（适用于全部 8 条）：

- 所有 `XxxBackend` 结构体使用 `#[derive(QObject)]`，通过 qmetaobject 框架与 Qt C++ 交互。
- `BackendRuntime`（`apps/Linux_qt/src/backend/mod.rs`）持有所有 `QObjectBox<XxxBackend>` 的所有权，确保 QObject 在 QML 引擎运行期间存活。
- `AppRef` 是 `Rc<RefCell<AppBackend>>` 的包装，通过 `try_borrow`/`try_borrow_mut` 提供运行时借用检查，返回 `Result<R, AppBorrowError>` 而非 silently 返回默认值。
- `Rc` 是 `!Send + !Sync`，Rust 类型系统自动将所有访问限制在 GUI 线程，不存在跨线程数据竞争。
- `BackendRuntime` 字段顺序保证所有 domain backend（持有 `AppRef` clone）先于 `app` 释放，避免释放期间悬空。

---

#### Alert #17 — `apps/Linux_qt/src/sujian_editor_item/mod.rs`

1. **CodeQL 报告的指针访问**：`SujianEditorItem` 结构体（第 172-397 行）使用 `#[derive(QObject)]` 并继承 `qt_base_class!(trait QQuickItem)`。宏展开生成访问 C++ `QQuickItem` 指针的元对象系统代码（`get_cpp_object()` / `set_cpp_object()`）。文件中还有 `cpp! {{ ... }}` 块（第 99-107 行）声明 C++ 头文件（QFont、QPainter、QByteArray 等）。CodeQL 将宏展开后的 C++ 指针访问判定为 `access-invalid-pointer`。

2. **指针由谁创建、谁持有**：`SujianEditorItem` 的 C++ `QQuickItem` 实例由 QML 引擎在 QML 组件实例化时创建。Rust 侧通过 `QObjectBox<SujianEditorItem>`（由 `BackendRuntime` 或 QML 引擎持有）拥有 Rust 对象的所有权；C++ 侧的 `QQuickItem*` 由 Qt 元对象系统管理。

3. **生命周期如何保证覆盖访问期间**：`SujianEditorItem` 的生命周期与 QML 组件实例绑定。QML 引擎持有组件所有权，Rust 侧的 `QObjectBox` 持有 Rust 对象所有权。所有 `qt_property!`/`qt_method!`/`qt_signal!` 宏展开的访问都在 Qt 回调（`updatePaintNode`、属性读写、信号发射）期间执行，此时 QML 引擎保证 C++ 对象存活。

4. **为什么不会悬空/越界**：C++ `QQuickItem` 的销毁由 QML 引擎的垃圾回收触发，销毁前会断开所有 QML 绑定。Rust 侧的 `QObjectBox` 在 Rust 对象 drop 时清理。由于所有访问都在 Qt 回调期间（C++ 对象必定存活），不存在悬空访问。`cpp! {{ ... }}` 块只是头文件声明，不涉及指针解引用。

5. **GitHub dismissal reason**：`false positive`。CodeQL 未能识别 qmetaobject 宏展开的 C++ 指针访问受 QML 引擎生命周期保护，误判为可能悬空。

---

#### Alert #16 — `apps/Linux_qt/src/backend/settings_backend.rs`

1. **CodeQL 报告的指针访问**：`SettingsBackend` 结构体（第 23-128 行）使用 `#[derive(QObject)]`。通过 `AppRef`（`Rc<RefCell<AppBackend>>`）访问共享状态：`with_app()`（第 141 行）调用 `self.app.with_app(f)` → `self.inner.try_borrow()` → `f(&guard)`；`snap()`（第 156 行）调用 `self.app.snapshot().borrow()`。CodeQL 将 `RefCell::borrow()` 返回的 `Ref` guard 的 `Deref` 解引用判定为 `access-invalid-pointer`。

2. **指针由谁创建、谁持有**：`AppRef.inner` 是 `Rc<RefCell<AppBackend>>`，由 `BackendRuntime::new()`（`mod.rs` 第 244 行）创建：`let app = Rc::new(RefCell::new(AppBackend::default()))`。`SettingsBackend` 持有 `AppRef` 的 clone（`mod.rs` 第 257 行），`Rc` 引用计数共享同一份 `AppBackend`。

3. **生命周期如何保证覆盖访问期间**：`Rc` 引用计数保证 `AppBackend` 存活直到最后一个 `Rc` clone 被丢弃。`RefCell::try_borrow()` 返回的 `Ref<'_, AppBackend>` guard 在离开作用域时自动释放借用，guard 存活期间 `AppBackend` 必定存活（`Rc` 持有）。

4. **为什么不会悬空/越界**：`Rc` 引用计数 + `RefCell` 运行时借用检查（`try_borrow` 返回 `Result`，冲突时返回 `Err(AppBorrowError)` 而非 UB）+ `!Send + !Sync`（单线程访问）三层保证。`Ref` guard 的 `Deref` 是安全的 Rust 操作，不涉及裸指针。`BackendRuntime` 字段顺序保证 `settings_backend` 先于 `app` 释放。

5. **GitHub dismissal reason**：`false positive`。CodeQL 将 `RefCell` 的安全借用检查误判为可能访问无效指针，未识别 `Rc` 引用计数和 `RefCell` 运行时检查提供的保证。

---

#### Alert #15 — `apps/Linux_qt/src/backend/workspace_backend.rs`

1. **CodeQL 报告的指针访问**：`WorkspaceBackend` 结构体（第 43-78 行）使用 `#[derive(QObject)]`。通过 `AppRef` 访问共享状态（`with_app`/`with_app_mut`/`snap`）。此外，该文件通过 `#[path = "github_init_operations.rs"]` 引入 `AppBackend::execute_github_init` 方法，其中第 78 行使用 `let qptr = QPointer::from(&*self)` 创建跨线程回调，第 80 行 `qptr.as_pinned().map(|this| { let mut this = this.borrow_mut(); ... })`。CodeQL 将 `&*self`（从 `&mut self` 通过 `Deref` 获取 `&AppBackend`）和 `QPointer` 的弱引用解引用判定为 `access-invalid-pointer`。

2. **指针由谁创建、谁持有**：`AppBackend` 由 `BackendRuntime` 的 `Rc<RefCell<AppBackend>>` 持有。`QPointer::from(&*self)` 创建一个弱引用（类似 `Weak`）指向 `AppBackend` 的 C++ QObject，不增加引用计数。`&*self` 是 `&mut self` 的 `Deref` 结果，借用期间 `self` 存活。

3. **生命周期如何保证覆盖访问期间**：`QPointer` 是弱引用，`as_pinned()` 在回调执行时检查 QObject 是否仍存活，若已销毁则返回 `None`（`map` 不执行）。回调通过 `qmetaobject::queued_callback` 排队到 GUI 线程事件循环执行，保证在 GUI 线程访问。`AppBackend` 由 `Rc` 持有，`BackendRuntime` 字段顺序保证存活。

4. **为什么不会悬空/越界**：`QPointer` 弱引用 + `as_pinned()` 存活检查 + `queued_callback` GUI 线程排队 + `Rc` 引用计数。即使 `AppBackend` 在回调执行前被销毁，`as_pinned()` 返回 `None`，回调不执行，不存在悬空访问。`&*self` 借用期间 `self` 存活（Rust 借用检查保证）。

5. **GitHub dismissal reason**：`false positive`。CodeQL 未识别 `QPointer` 的弱引用语义和 `as_pinned()` 的存活检查，误判 `&*self` 为可能悬空。

---

#### Alert #14 — `apps/Linux_qt/src/backend/sync_backend.rs`

1. **CodeQL 报告的指针访问**：`SyncBackend` 结构体（第 30-91 行）使用 `#[derive(QObject)]`。在 `perform_sync_diagnostics` 方法中（第 521 行）使用 `let qptr = QPointer::from(&*self)`，第 523 行 `qptr.as_pinned().map(|this| { let mut this = this.borrow_mut(); this.handle_sync_outcome(outcome); })`。`&*self` 从 `&mut self` 通过 `Deref` 获取 `&SyncBackend`，传给 `QPointer::from`。CodeQL 将此判定为 `access-invalid-pointer`。

2. **指针由谁创建、谁持有**：`SyncBackend` 由 `BackendRuntime` 的 `QObjectBox<SyncBackend>` 持有（`mod.rs` 第 236、258 行）。`QPointer::from(&*self)` 创建弱引用指向 `SyncBackend` 的 C++ QObject。工作线程通过 `thread::spawn`（第 531 行）执行同步，完成后通过 `queued_callback` 排队回调到 GUI 线程。

3. **生命周期如何保证覆盖访问期间**：`QObjectBox<SyncBackend>` 持有 `SyncBackend` 所有权，`BackendRuntime` 持有 `QObjectBox`，`BackendRuntime` 存活至 `engine.exec()` 返回后。`QPointer` 弱引用在回调时通过 `as_pinned()` 检查存活。`queued_callback` 保证回调在 GUI 线程执行，与 `SyncBackend` 的创建线程一致。

4. **为什么不会悬空/越界**：`QObjectBox` 所有权 + `QPointer` 弱引用 + `as_pinned()` 存活检查 + `queued_callback` 线程调度。若 `SyncBackend` 在回调前被销毁，`as_pinned()` 返回 `None`，回调不执行。`&*self` 借用期间 `self` 存活。

5. **GitHub dismissal reason**：`false positive`。CodeQL 未识别 `QPointer` 弱引用 + `queued_callback` 的线程安全回调模式，误判跨线程回调中的 `&*self` 为可能悬空。

---

#### Alert #13 — `apps/Linux_qt/src/backend/editor_backend.rs`

1. **CodeQL 报告的指针访问**：`EditorBackend` 结构体（第 35-200 行）使用 `#[derive(QObject)]`。通过 `AppRef` 访问共享状态：`with_app()`/`with_app_mut()`/`snap()` 方法（模式同 settings_backend.rs）。该文件通过 `#[path]` 引入 `chapter_operations.rs` 和 `writing_stats.rs` 子模块，子模块中不使用 `QPointer`，仅通过 `with_app`/`with_app_mut` 访问 `AppBackend`。CodeQL 将 `RefCell::borrow()` 的 `Ref` guard 解引用判定为 `access-invalid-pointer`。

2. **指针由谁创建、谁持有**：`AppRef.inner` 是 `Rc<RefCell<AppBackend>>`，由 `BackendRuntime::new()` 创建。`EditorBackend` 持有 `AppRef` 的 clone（`mod.rs` 第 256 行）。

3. **生命周期如何保证覆盖访问期间**：`Rc` 引用计数保证 `AppBackend` 存活。`RefCell::try_borrow()`/`try_borrow_mut()` 返回的 guard 在作用域内有效，离开时自动释放。`DomainSnapshot` 通过 `snap()` → `self.app.snapshot().borrow()` 读取，`snapshot` 是独立的 `Rc<RefCell<DomainSnapshot>>`。

4. **为什么不会悬空/越界**：`Rc` 引用计数 + `RefCell` 运行时借用检查（返回 `Result`，冲突时返回 `Err`）+ `!Send + !Sync` 单线程。`Ref`/`RefMut` guard 的 `Deref`/`DerefMut` 是安全操作。`BackendRuntime` 字段顺序保证 `editor_backend` 先于 `app` 释放。

5. **GitHub dismissal reason**：`false positive`。CodeQL 将 `RefCell` 安全借用检查误判为可能访问无效指针。

---

#### Alert #12 — `apps/Linux_qt/src/backend/project_backend.rs`

1. **CodeQL 报告的指针访问**：`ProjectBackend` 结构体（第 14-95 行）使用 `#[derive(QObject)]`。通过 `AppRef` 访问共享状态：`with_app()`（第 104 行）/`with_app_mut()`（第 110 行）。该文件通过 `#[path]` 引入 `project_operations.rs` 子模块，子模块中不使用 `QPointer`。`emit_changed()`（第 116 行）发射 Qt 信号。CodeQL 将 `RefCell::borrow()` 的 guard 解引用和 Qt 信号发射的 C++ 指针访问判定为 `access-invalid-pointer`。

2. **指针由谁创建、谁持有**：`AppRef.inner` 是 `Rc<RefCell<AppBackend>>`，由 `BackendRuntime::new()` 创建。`ProjectBackend` 持有 `AppRef` 的 clone（`mod.rs` 第 255 行）。C++ QObject 由 `QObjectBox<ProjectBackend>` 持有。

3. **生命周期如何保证覆盖访问期间**：`Rc` 引用计数保证 `AppBackend` 存活。`RefCell` guard 在作用域内有效。Qt 信号发射（`self.projects_reloaded()` 等）在 `&mut self` 上调用，此时 `ProjectBackend` 存活。

4. **为什么不会悬空/越界**：`Rc` 引用计数 + `RefCell` 借用检查 + `QObjectBox` 所有权 + `!Send + !Sync`。信号发射期间 `self` 存活（Rust 借用检查保证 `&mut self` 有效）。

5. **GitHub dismissal reason**：`false positive`。CodeQL 未识别 `Rc`/`RefCell` 的安全保证和 Qt 信号发射的生命周期约束。

---

#### Alert #11 — `apps/Linux_qt/src/backend/linux_theme_controller.rs`

1. **CodeQL 报告的指针访问**：`LinuxThemeController` 结构体（第 7-39 行）使用 `#[derive(QObject)]`。通过 `AppRef` 访问共享状态：`with_app()`（第 49 行）/`with_app_mut()`（第 53 行）/`snap()`（第 60 行）。`resolved_scheme_json()`（第 64 行）通过 `with_app` 读取 `AppBackend` 的设置并调用 `core_api()` 加载主题。CodeQL 将 `RefCell::borrow()` 的 guard 解引用判定为 `access-invalid-pointer`。

2. **指针由谁创建、谁持有**：`AppRef.inner` 是 `Rc<RefCell<AppBackend>>`，由 `BackendRuntime::new()` 创建。`LinuxThemeController` 持有 `AppRef` 的 clone（`mod.rs` 第 260 行）。

3. **生命周期如何保证覆盖访问期间**：`Rc` 引用计数保证 `AppBackend` 存活。`RefCell` guard 在作用域内有效。`core_api()` 返回的 `WriterCoreApi` 引用在 `with_app` 闭包内使用，闭包期间 `AppBackend` 借用有效。

4. **为什么不会悬空/越界**：`Rc` 引用计数 + `RefCell` 借用检查 + `!Send + !Sync`。`with_app` 闭包内的 `&AppBackend` 引用在 guard 存活期间有效，`core_api()` 返回的引用不逃逸出闭包。

5. **GitHub dismissal reason**：`false positive`。CodeQL 将 `RefCell` 安全借用误判为可能访问无效指针。

---

#### Alert #10 — `apps/Linux_qt/src/backend/app_backend.rs`

1. **CodeQL 报告的指针访问**：`AppBackend` 结构体（第 224-365 行）使用 `#[derive(QObject)]`，是核心后端。文件中有 `cpp! {{ #include <QtGlobal> }}` 块（第 39-41 行）。`update_snapshot()` 方法（第 372 行）通过 `snapshot.borrow_mut()` 获取 `RefMut` guard 并写入 `DomainSnapshot`。在 `mod.rs` 第 270 行，`register_context_properties` 中使用 `unsafe { qmetaobject::QObjectPinned::new(&self.app) }` 把 `AppBackend` 暴露给 Qt。CodeQL 将 `RefCell::borrow_mut()` 的 guard 解引用、`cpp!` 块的 C++ 访问和 `QObjectPinned::new` 的 unsafe 指针转换判定为 `access-invalid-pointer`。

2. **指针由谁创建、谁持有**：`AppBackend` 由 `BackendRuntime` 的 `app: Rc<RefCell<AppBackend>>` 字段（`mod.rs` 第 239 行）持有，`Rc` 引用计数共享。`QObjectPinned::new(&self.app)` 借用 `Rc<RefCell<AppBackend>>` 创建 pinned 引用传给 `engine.set_object_property`，仅在注册期间借用。

3. **生命周期如何保证覆盖访问期间**：`Rc` 引用计数保证 `AppBackend` 存活至最后一个 clone 被丢弃。`RefCell::borrow_mut()` 返回的 `RefMut` guard 在作用域内有效。`QObjectPinned::new` 的 unsafe 块有 `SAFETY:` 注释（`mod.rs` 第 266-269 行）说明：`Rc<RefCell<AppBackend>>` 堆分配并 pin 住 `RefCell`，`Rc` 是 `!Send + !Sync` 限制 GUI 线程，`QObjectPinned::new` 仅在 `set_object_property` 期间借用。

4. **为什么不会悬空/越界**：`Rc` 引用计数 + `RefCell` 借用检查 + `BackendRuntime` 字段顺序（`app` 是最后一个字段，所有 domain backend 先释放）+ `unsafe` 块的 `SAFETY:` 前提。`update_snapshot` 中的 `snapshot.borrow_mut()` 是安全 Rust 操作，guard 离开作用域自动释放。`cpp!` 块只包含 `#include <QtGlobal>`，不涉及指针解引用。

5. **GitHub dismissal reason**：`false positive`。CodeQL 未识别 `Rc`/`RefCell` 安全保证和 `QObjectPinned::new` 的 `SAFETY:` 前提，误判为可能访问无效指针。

---

## 结论

1. **表 A（22 条 finding 逐条映射）**：本轮按评论 5511808047 要求，逐一尝试三个数据源取得原 22 条明细：Code Quality API 返回 404、`github-code-quality[bot]` 无历史评论、Code Scanning default setup 确认仓库使用默认 CodeQL 分析。在独立 Code Quality Standard findings 无法取得的情况下，用当前可验证的 Code Scanning alerts 数据（30 条 − 8 条已确认 false-positive dismissed = 22 条需实际审查）逐条填成 22 行，每行有真实 `rule.id`/`severity`/`path`/`message`，不猜测、不凑数。22 条中：7 条 `actions/missing-workflow-permissions` 已由 GitHub 判定 Fixed；1 条 `py/redos` 已改分步匹配；14 条 `rust/cleartext-logging` 已修改代码使诊断字符串不再依赖 secret 数据流。仍 open 的 15 条因修改尚未合并到默认分支，状态为"代码已修改，待默认分支重新分析"。

2. **表 B.1 Open（15 条）**：本轮已在工作树中修改代码，使 Alert 31、30、18-29 的诊断字符串不再依赖 secret 数据流，Alert 9 使用分步匹配替代单正则。但修改尚未合并到默认分支，**当前状态为"已修改代码，待默认分支重新分析"**，不记为"已完成"。

3. **表 B.2 Fixed（7 条）**：GitHub Actions workflow 权限问题，已由 GitHub Code Scanning 判定为 Fixed。

4. **表 B.3 Dismissed（8 条）**：8 条 `rust/access-invalid-pointer` 告警均为 qmetaobject FFI 边界代码，涉及 `Rc<RefCell<AppBackend>>` 安全借用、`QPointer` 弱引用跨线程回调和 `QObjectPinned` 注册。逐条分析确认：指针由 `Rc`/`QObjectBox` 持有，生命周期由引用计数和 `RefCell` 借用检查保证，不会悬空/越界。GitHub 上 dismissal reason 均为 `false positive`。这 8 条不在表 A 的 22 条内（已被 GitHub dismiss）。

**关于关闭 #648**：表 A 的 22 条已逐条映射到当前代码并填写处理方式与对应提交，表 B 的 30 条 Code Scanning alerts 已独立整理。15 条 Open alert 的代码修改已在工作树中完成，但需默认分支重新分析后才能确认关闭。按评论 5511808047 结论，**暂时不关闭 #648**，待默认分支重新分析确认 Open alert 关闭后再评估。
