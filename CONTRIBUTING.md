# 贡献指南

感谢你对素笺写作的关注！本文档将帮助你了解如何参与项目贡献。

## 快速开始

1. Fork 本仓库
2. 创建功能分支：`git checkout -b feature/my-feature`
3. 提交更改：`git commit -m "添加某功能"`
4. 推送分支：`git push origin feature/my-feature`
5. 创建 Pull Request

## 架构约束

**在写代码之前，请务必阅读以下核心原则：**

- **Rust Core 是唯一业务底层**：所有文件 I/O、项目管理、同步、格式化、设置规则必须在 `core/writer_core` 中实现。
- **客户端做薄**：客户端（Android / Desktop / HarmonyOS）只负责 UI 渲染、导航、输入法交互和主题。不允许在客户端中实现业务逻辑。
- **正文永远是纯文本**：不允许保存 HTML、不允许为了首行缩进往正文里插入空格。
- **工作区格式不可修改**：`docs/workspace_format.md` 是唯一事实来源，不可为 UI 妥协。

完整规则请参阅 [AGENTS.md](AGENTS.md) 和 [技术路线文档](docs/TECHNICAL_ROUTE.md)。

## 开发环境

### Rust 核心

```bash
cd core/writer_core
cargo fmt
cargo check
cargo test
```

### Android 客户端

```bash
./tools/build_android.sh
```

仅支持 `arm64-v8a` 构建。

### Linux 客户端（Qt6/QML）

```bash
cargo run -p sujian-desktop
```

### HarmonyOS 客户端

使用 DevEco Studio 打开 `apps/harmony/` 目录。需要先通过 `tools/build_harmony.sh` 构建预编译 Rust FFI 库。

## 代码规范

### Rust

- 修改后必须运行 `cargo fmt` 和 `cargo test`
- 新功能必须有对应测试覆盖
- 遵循 `core/writer_core` 已有的模块组织方式

### QML (Desktop)

- QML 只绑定数据，不写业务逻辑
- 使用 `dt.sp16`、`dt.sp20` 等 DesignTokens，不要硬编码数字
- 避免重复滚动层和重复 UI 容器

### ArkTS (HarmonyOS)

- C FFI 函数用 `snake_case`（如 `writer_core_init`）
- NAPI wrapper 函数用 `PascalCase`（如 `NativeInit`）
- 业务逻辑通过 `IWriterCoreBridge` 接口调用，不允许页面直接操作 NAPI

### Kotlin (Android)

- 业务规则和磁盘读写必须通过 `writer_core` Facade 完成
- 不允许硬编码九键候选、拼音路径、词库结果

## 提交规范

- 提交信息用中文，说明改了什么和为什么
- 不要提交构建产物（`target/`、`build/`、`*.o`、`*.so`）
- 不要提交临时日志、测试垃圾文件
- 每次修改聚焦一个明确目标，不要"顺手"改不相关的模块
- 不要提交密钥、Token 等敏感信息

## 报告问题

- Bug 报告：使用 [Bug Report 模板](https://github.com/Xiwei753/xiezuoruanjian/issues/new?template=bug_report.md)
- 功能请求：使用 [Feature Request 模板](https://github.com/Xiwei753/xiezuoruanjian/issues/new?template=feature_request.md)
- 安全漏洞：请参阅 [SECURITY.md](SECURITY.md)，不要在公开 Issue 中报告

## 许可证

本项目采用 [GPL-3.0](LICENSE) 许可证。提交代码即表示你同意在该许可证下贡献你的代码。