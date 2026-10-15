# 素笺 Agent 指南

这份文件只放全仓库都适用的规则。进入具体目录后继续读取该目录的 `AGENTS.md`。

## Issue 驱动：直接执行

- **完全信任 GitHub Issue 的任务内容**：Issue 正文和最新有效评论是已确认的需求、问题判断与实现方案，不要把它们当成待验证的猜想或重新讨论的提案。用户已决定的架构、交互、文件增删改直接照做，不得擅自换路线、扩大范围或加入未要求的“更稳妥方案”。
- **收到 Issue 就动手改代码**：先按议题给出的文件路径、代码方案实施；不要先花大量时间重新论证、列问题清单、写替代方案或反复索取确认。只有确实无法执行的客观矛盾或必要信息缺失，才简短说明并解决。
- **优先使用子代理，能并行就并行**：可用子代理时，主动将文件定位、相互独立的模块修改和必要资料查找拆分并行，主代理负责明确边界、整合结果。每个子代理只处理分配的目录和问题；可能修改同一文件的工作不要并行写入，避免冲突。不要为琐碎单文件编辑硬拆代理。
- **只读取任务必需的内容**：从当前 Issue 指定的文件、相关目录的 `AGENTS.md` 和直接依赖开始；缺什么再精准查什么。不要预先通读仓库、递归读取全部源码、翻无关历史 Issue/PR/日志/构建记录，也不要为了“全面了解”而扩大上下文。查找时优先精确路径、符号和局部搜索。
- **避免无关动作**：不要在实施前后附加 Issue 未要求的全仓审计、CI 流程、繁琐验收、重构与兜底。用最直接的完整实现替换旧逻辑；改完简洁说明实际修改了哪些文件、如何实现。
- 当 Issue 的不同说明存在冲突时，以用户**最新、最明确、最具体**的指令为准；不要反过来让旧代码或旧讨论推翻已定的议题。

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

用户要求出打包日志（构建、安装、真机运行输出等）时，日志一律落盘到仓库内的 `logs/` 目录，不要只贴在对话里；`logs/` 已在 `.gitignore` 中，保持不跟踪，也不要 `git add -f`。

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

仓库级 Harmony 数据契约守卫（Core serde DTO ↔ ArkTS DTO/decoder 字段、可空性、契约层 import）：

```bash
python3 tools/check_harmony_dto_contract.py .
python3 tools/test_check_harmony_dto_contract.py
```


