# 素笺 (writer) Codebase 文档

> 📁 本文档为项目总览。这是一个 Rust workspace 项目，业务核心在 `core/writer_core`。

## 1. 技术栈

- **语言**：Rust (edition 2021)
- **构建工具**：Cargo (workspace, resolver = "2")
- **核心 crate**：`writer_core` (rlib)、`writer_platform_api`、`writer_uniffi`
- **FFI 绑定**：UniFFI 0.28.0
- **关键依赖**：serde 1.0 / serde_json 1.0 / chrono 0.4 / uuid 1.8 / git2 0.20.4 / libgit2-sys 0.18 (vendored) / tempfile 3.8 / walkdir 2.3 / rayon 1.12 / crop 0.4.3
- **测试**：cargo test (内联 #[test] + tests/ 目录集成测试)
- **Lint**：workspace 级 clippy (unwrap_used/expect_used/cast_*/cognitive_complexity 等均为 warn)
- **平台目标**：Linux Qt / Android / Harmony (各平台在 platform/rust/<target>/)

## 2. 项目目录结构

```
auto-issue-645-comment-5504296097-20260905012937/
├── core/                                # 业务核心与平台契约
│   ├── writer_core/                     # 业务真相唯一事实来源（作品/卷/章节/正文/同步/统计/星图）
│   │   ├── src/                         # 核心源码，涵盖 api/facade/project/volume/chapter/editor/sync/starmap/storage 等
│   │   ├── tests/                       # 集成测试（含 issue 复现测试）
│   │   ├── benches/                     # criterion 基准测试
│   │   └── Cargo.toml
│   ├── writer_platform_api/             # Core 所需的平台能力契约
│   └── writer_uniffi/                   # 稳定的 UniFFI 导出门面
├── platform/rust/<target>/             # 各平台 Rust 适配与最终动态库组装 (linux/android/harmony)
├── apps/                                # 平台客户端
│   ├── android/                         # Android 原生 Kotlin/Compose 客户端
│   └── Linux_qt/                        # Linux Qt 客户端
├── docs/                                # 长期架构、数据格式和跨平台契约
├── tools/                               # 仓库工具脚本（含 Rust 安全守卫）
├── bindings/                            # 自动生成的绑定产物
├── fixtures/                            # 测试 fixtures
├── scripts/                             # 构建/发布脚本
├── packaging/                           # 打包配置
├── site/                                # 站点资源
├── .github/                             # CI 配置与 githooks
├── Cargo.toml                           # workspace 根
└── Cargo.lock
```

## 3. 开发命令（核心）

### 构建命令
```bash
# 构建 writer_core（来源：core/writer_core/AGENTS.md > 常用命令）
cargo build -p writer_core
```

### 测试命令
```bash
# 通用 Core 测试（来源：core/writer_core/AGENTS.md > 常用命令）
cargo test -p writer_core

# AI 专项测试（来源：core/writer_core/AGENTS.md > 常用命令）
cargo test -p writer_core --features ai --test ai_feature

# 单个测试文件（来源：Rust cargo test 通用用法）
cargo test -p writer_core --test issue_645_comment_5504296097_repro
```

### Lint / 格式化命令
```bash
# 完整 Core Clippy（来源：core/writer_core/AGENTS.md > 常用命令）
cargo clippy --package writer_core --all-targets --all-features -- -D warnings

# 格式检查（来源：core/writer_core/AGENTS.md > 常用命令）
cargo fmt --all --check

# Rust 安全守卫（来源：仓库根 AGENTS.md > Rust 安全边界）
python3 tools/check_rust_safety_patterns.py .
python3 tools/test_check_rust_safety_patterns.py
```

### 运行 / 启动命令
```bash
# 文档中未提供统一启动命令，各平台客户端独立启动（见 apps/<platform>/）
```

## 4. 开发环境（简要）

### 前置要求

- Rust toolchain (edition 2021，建议 stable 最新版)
- Cargo (workspace 支持)
- 各平台原生工具链（按需：Android NDK / Qt / Harmony SDK）

### 配置说明

- `Cargo.toml` workspace 根，统一 lint 配置
- `.cargo/` 配置 cargo 行为
- `clippy.toml` clippy 专属配置
- `core/writer_core/Cargo.toml` 中 `git-https` / `github-api` / `harmony-ffi` 为可选 feature

## 5. 关键模块说明（writer_core）

- `api/`：稳定 DTO、错误和服务边界（含 `api/service/project_ops.rs` 项目操作 API 层）
- `facade/`：Core 的业务入口与聚合服务（含 `facade/project_ops.rs`）
- `project/`、`volume/`、`chapter/`：作品结构和正文持久化
- `editor/`：正文事务、revision 和编辑意图
- `sync/`：同步协议和状态机
- `starmap/`：星图业务数据与引用语义
- `storage/`：原子写入、workspace git、workspace 路径分类（含 `workspace_paths.rs`）
- `storage/journal/`：删除 journal（含 `project_delete.rs`）
- `delete_guard/`：删除边界
- `settings/`：设置规则
- `writing_stats/`：统计
- `history/`：本地 Git history 集成
