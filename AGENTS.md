# 素笺 Agent 指南

这份文件只放全仓库都适用的规则。进入具体目录后继续读取该目录的 `AGENTS.md`。

## 仓库地图

- `core/writer_core/`：业务核心。作品、卷、章节、正文、设置、同步、统计、星图和数据格式的唯一事实来源。
- `core/writer_platform_api/`：Core 所需的平台能力契约。
- `core/writer_uniffi/`：稳定的 UniFFI 导出门面。
- `platform/rust/<target>/`：各平台 Rust 适配与最终动态库组装。
- `apps/android/`：Android 原生 Kotlin/Compose 客户端。
- `apps/Linux_qt/`：Linux Qt 客户端。
- `docs/`：长期架构、数据格式和跨平台契约。

Rust workspace 以根目录 `Cargo.toml` 为准，不要自行猜 crate 关系。

## 全局边界

- Core 保存业务真相；平台端负责 UI、导航、输入法、排版、渲染、动画和系统能力。
- 平台端不得复制作品、章节、设置、同步、统计等业务状态机。
- Core 不依赖 Android、Qt、Compose、QML、Activity、View、Window 等平台类型。
- 正文持久化始终是纯文本。预输入、选区、缩进显示、动画、高亮都不是正文格式。
- 跨语言接口优先强类型 DTO、枚举和错误类型；不要在 Rust 内部把结构体转 JSON 再解析回来。
- 自动生成的 UniFFI/JNI/NAPI 绑定不手工修改；生成结果错误时修生成契约或上游接口。
- 被新实现替代的旧入口直接删除，不再增加 fallback、兼容旁路或第二套状态机。

## 修改位置

改业务规则、数据格式、同步语义、编辑事务：先看 `core/writer_core/AGENTS.md`。

改 Android UI、输入、动画、窗口、系统能力、Gradle：先看 `apps/android/AGENTS.md`。

长期架构以 `docs/TECHNICAL_ROUTE.md` 为准；数据目录、设置、同步、星图语义分别看对应文档。一次性迁移步骤和文件级改法写 Issue，不写进长期文档。

## Rust 安全边界

- 不为通过编译手写 `unsafe impl Send/Sync`。
- 不从共享引用伪造 `&mut`，不靠裸指针、`Box::leak`、`mem::forget` 解决生命周期问题。
- `unsafe` 只放必要的 FFI/平台边界，并紧邻写明 `SAFETY:` 前提。
- 外部输入、锁、磁盘、网络路径不用 `unwrap`/`expect` 代替错误处理。
- 不用 crate/module 级宽范围 `allow` 掩盖 warning。

仓库级 Rust 守卫：

```bash
python3 tools/check_rust_safety_patterns.py .
python3 tools/test_check_rust_safety_patterns.py
```
