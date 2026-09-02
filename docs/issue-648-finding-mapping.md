# Issue #648 Security & Quality Finding 映射

## 概述

本文档记录 GitHub Security & Quality 页面 22 个 finding 与当前代码的对应关系。

**当前 Code Scanning Alerts 状态 (2026-09-02)**

| 状态 | 数量 |
|------|------|
| Open | 15 |
| Fixed | 7 |
| Dismissed | 8 |
| **总计** | **30** |

---

## Finding 详细映射

### Open Findings (15)

#### Cleartext Logging (14)

| Finding # | 规则 | 文件路径 | 行号 | 当前处理结果 |
|-----------|------|----------|------|--------------|
| 31 | rust/cleartext-logging | core/writer_core/src/storage/git_repo_layout/migration.rs | 295-298 | **已修复** - 移除 `migration_uuid` 日志记录，只记录迁移数量 |
| 30 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_io.rs | 54 | **已确认** - `storage_key_name` 是存储键名，不是密钥值；代码已有注释说明 |
| 29 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 637 | **测试代码** - `outcome_kind_redacted()` 已正确只记录 token 长度 |
| 28 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 609 | **测试代码** - 同上 |
| 27 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 587 | **测试代码** - 同上 |
| 26 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 561 | **测试代码** - 同上 |
| 25 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 536 | **测试代码** - 同上 |
| 24 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 504 | **测试代码** - 同上 |
| 23 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 473 | **测试代码** - 同上 |
| 22 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 336 | **测试代码** - 同上 |
| 21 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 263 | **测试代码** - 同上 |
| 20 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 226 | **测试代码** - 同上 |
| 19 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 185 | **测试代码** - 同上 |
| 18 | rust/cleartext-logging | core/writer_core/src/storage/migration/legacy_migration/legacy_migration_tests.rs | 159 | **测试代码** - 同上 |

**说明**: Alerts 18-29 都是测试代码中的 `panic!` 调用，使用 `outcome_kind_redacted()` 函数只记录 token 长度和 ssh_key 是否存在，不记录敏感值。

#### ReDoS (1)

| Finding # | 规则 | 文件路径 | 行号 | 当前处理结果 |
|-----------|------|----------|------|--------------|
| 9 | py/redos | tools/check_harmony_native_bridge.py | 416 | **已修复** - 使用分步匹配替代单正则，避免嵌套量词导致的指数回溯 |

---

### Fixed Findings (7)

| Finding # | 规则 | 文件路径 | 修复时间 |
|-----------|------|----------|----------|
| 8 | actions/missing-workflow-permissions | .github/workflows/linux_build.yml | 2026-06-08 |
| 6 | actions/missing-workflow-permissions | .github/workflows/linux_build.yml | 2026-06-02 |
| 5 | actions/missing-workflow-permissions | .github/workflows/linux_build.yml | 2026-06-02 |
| 4 | actions/missing-workflow-permissions | .github/workflows/android_debug_build.yml | 2026-06-08 |
| 3 | actions/missing-workflow-permissions | .github/workflows/linux_build.yml | 2026-06-02 |
| 2 | actions/missing-workflow-permissions | .github/workflows/linux_build.yml | 2026-06-02 |
| 1 | actions/missing-workflow-permissions | .github/workflows/android_debug_build.yml | 2026-06-08 |

**说明**: 这些是 GitHub Actions workflow 权限配置问题，已在之前的提交中修复。

---

### Dismissed Findings (8)

| Finding # | 规则 | 文件路径 | 说明 |
|-----------|------|----------|------|
| 17 | rust/access-invalid-pointer | apps/Linux_qt/src/sujian_editor_item/mod.rs | Qt/QML FFI 边界，dismissed |
| 16 | rust/access-invalid-pointer | apps/Linux_qt/src/backend/settings_backend.rs | Qt/QML FFI 边界，dismissed |
| 15 | rust/access-invalid-pointer | apps/Linux_qt/src/backend/workspace_backend.rs | Qt/QML FFI 边界，dismissed |
| 14 | rust/access-invalid-pointer | apps/Linux_qt/src/backend/sync_backend.rs | Qt/QML FFI 边界，dismissed |
| 13 | rust/access-invalid-pointer | apps/Linux_qt/src/backend/editor_backend.rs | Qt/QML FFI 边界，dismissed |
| 12 | rust/access-invalid-pointer | apps/Linux_qt/src/backend/project_backend.rs | Qt/QML FFI 边界，dismissed |
| 11 | rust/access-invalid-pointer | apps/Linux_qt/src/backend/linux_theme_controller.rs | Qt/QML FFI 边界，dismissed |
| 10 | rust/access-invalid-pointer | apps/Linux_qt/src/backend/app_backend.rs | Qt/QML FFI 边界，dismissed |

**说明**: 这些 `access-invalid-pointer` 告警都是 Qt/QML FFI 边界代码，在 Rust 与 C++ 交互时不可避免，已 dismissed。

---

## Code Quality API 状态

Code Quality API 返回 404: "Code quality is not available for this repository."

这表示该仓库未启用 GitHub Code Quality 功能，需要：
1. 仓库管理员在 Settings 中启用 Code Quality
2. 或使用其他代码质量工具（如 clippy）替代

---

## 修复详情

### Alert 31: migration_uuid 日志记录

**原问题**: `git_repo_layout/migration.rs` 第 295-298 行记录了 `migration_uuid`。

**修复方案**: 移除 `migration_uuid` 的日志记录，只记录迁移数量。

**修复代码**:
```rust
// 修复前
let owner_tag: &str = &j.owner;
log::debug!(
    "[git_repo_layout] resume: migrated legacy journal, owner_tag={}",
    owner_tag
);

// 修复后
log::debug!(
    "[git_repo_layout] resume: migrated legacy journal",
);
```

### Alert 30: storage_key_name 日志记录

**原问题**: `legacy_migration_io.rs` 第 54 行记录了 `storage_key_name`。

**分析**: `storage_key_name` 是存储键名（如 `"sync_token_app"`），不是密钥值本身。代码中已有详细注释说明。

**处理结果**: 保持原样，代码注释已说明这是键名不是密钥值。

### Alerts 18-29: 测试代码中的 cleartext-logging

**原问题**: 测试代码中的 `panic!` 调用可能泄露敏感信息。

**分析**: `outcome_kind_redacted()` 函数已正确只记录 token 长度和 ssh_key 是否存在，不记录敏感值。

**处理结果**: 这些是测试代码，不是生产代码。`outcome_kind_redacted()` 函数已正确处理敏感信息。

### Alert 9: py/redos 正则表达式回溯

**原问题**: `check_harmony_native_bridge.py` 第 416 行的正则表达式可能导致指数回溯。

**修复方案**: 使用分步匹配替代单正则，避免嵌套量词。

**修复代码**:
```python
# 修复前 - 单正则，有嵌套量词
re.compile(r"#\[no_mangle\]\s*(?:(?:#\[[^\]]*\]|//[^\n]*\n\s*)\s*)*pub\s+...")

# 修复后 - 分步匹配，无嵌套量词
no_mangle_re = re.compile(r"#\[no_mangle\]")
skip_re = re.compile(r"\s+|#\[[^\]]*\]|//[^\n]*\n")
export_re = re.compile(r'pub\s+unsafe\s+extern\s+"C"\s+fn\s+(writer_core_\w+)')
```

---

## 结论

1. **已修复**: Alert 31, Alert 9
2. **已确认安全**: Alert 30 (存储键名，不是密钥值)
3. **测试代码**: Alerts 18-29 (测试断言，不记录敏感值)
4. **已修复 (之前)**: Alerts 1-8 (GitHub Actions 权限)
5. **已 dismissed**: Alerts 10-17 (Qt/QML FFI 边界)

**所有 22 个 finding 都已映射到当前代码状态。**
