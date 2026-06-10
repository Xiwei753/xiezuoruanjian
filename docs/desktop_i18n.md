# Desktop Client i18n Guide

## 1. 原则

- 所有 Desktop 客户端 QML 用户可见文本必须使用 `qsTr("文本")` 包裹。
- 不要在 QML 中使用硬编码的中文拼接，使用 `.arg()` 机制：`qsTr("删除「%1」吗？").arg(title)`。
- i18n 机制采用 Qt 官方的 `.ts` 翻译文件系统。

## 2. 当前进度状态

> [!WARNING]
> **当前阶段只完成 qsTr 标记，运行时翻译加载未完成。**
> 测试用例仅验证代码中是否对中文进行了 `qsTr()` 标记，目前 `main.rs` 并未加载 `.qm` 文件，真正的多语言切换在 UI 上不生效。请勿在开发过程中声称 i18n 已完整完成。

## 3. 工具链

- **lupdate**：从 `.qml` 源码提取待翻译词条并生成/更新 `.ts` 文件。
  ```bash
  lupdate apps/desktop/qml -ts apps/desktop/i18n/writer_zh_CN.ts
  ```
- **lrelease**：将 `.ts` 文件编译为 Qt 运行时高效加载的 `.qm` 文件。
  ```bash
  lrelease apps/desktop/i18n/writer_zh_CN.ts -qm apps/desktop/i18n/writer_zh_CN.qm
  ```

## 3. 在 Rust/Qt 代码中加载翻译

在 `main.rs` 或其他加载 QML 引擎的代码处：

```rust
// 伪代码示例，依赖具体 C++ 或 Rust qmetaobject/qml 绑定库的支持
// engine.load_translator(":/i18n/writer_zh_CN.qm");
```

## 4. 添加新语言

1. 拷贝现有 `.ts`：`cp writer_zh_CN.ts writer_en_US.ts`
2. 用 Qt Linguist 打开翻译，或手动修改 XML 内容。
3. 编译出 `.qm` 并在运行时根据用户选择的语言加载。
