# Linux_qt 多语言 (i18n) 路线说明

## 当前状态

Linux_qt 客户端采用 **中文 source + zh_CN catalog** 模式：

- QML 中所有用户可见中文文本必须用 `qsTr("中文")` 包裹
- `lupdate` 扫描 QML 源码生成 `zh_CN.ts`（XML 翻译源文件）
- `lrelease` 将 `.ts` 编译为 `.qm`（二进制翻译文件）
- `main.rs` 启动时通过 `QTranslator` 加载 `:/i18n/zh_CN.qm`
- source 和 translation 内容相同（中文 → 中文），因为 sourcelanguage 就是 zh_CN

## 完整流程

```
QML 源码 (qsTr)
    ↓ lupdate
zh_CN.ts (翻译源文件)
    ↓ lrelease
zh_CN.qm (编译后翻译文件)
    ↓ QTranslator::load()
QML 运行时 retranslate
```

## 新增语言步骤

1. 在 `apps/Linux_qt/i18n/` 下创建新的 `.ts` 文件（如 `en.ts`）
2. 运行 `lupdate` 更新所有 `.ts` 文件
3. 翻译新 `.ts` 文件中的 `<translation>` 条目
4. 运行 `lrelease` 编译所有 `.ts` → `.qm`
5. 在 `main.rs` 的 qrc 中注册新 `.qm` 文件
6. 修改 `install_translator()` 函数，根据用户语言偏好加载对应 translator
7. 设置页添加语言选项，切换时：
   - 卸载旧 translator：`QCoreApplication::removeTranslator(old)`
   - 安装新 translator：`QCoreApplication::installTranslator(new)`
   - 触发 QML retranslate（通过 `retranslate()` 信号或重新加载）

## 检查工具

- `python3 tools/check_i18n.py` — 检查 QML qsTr 覆盖率、zh_CN.ts 完整性
- `cargo test -p sujian-linux-qt qml_static_check` — Rust 集成测试，含中文必须 qsTr 检查

## 注意事项

- Core/Rust 不应直接暴露最终 UI 中文文案，应使用 i18n key + fallback 模式
- Android 端使用 `strings.xml`，Kotlin 代码不应硬编码中文字符串
- `check_i18n.py` 和 `check_android_i18n.py` 已接入 CI，违规会导致构建失败
