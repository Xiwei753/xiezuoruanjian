# writer_core Agent 指南

这里是素笺的业务核心。全局规则先看仓库根目录 `AGENTS.md`。

## 负责什么

`writer_core` 是以下数据和规则的唯一事实来源：

- 作品、卷、章节与正文；
- 编辑事务与正文 revision；
- 设置 Schema；
- 同步计划、删除和冲突语义；
- 写作统计；
- 星图数据与引用语义；
- 数据目录格式和安全删除规则。

平台端可以展示这些状态，但不能复制一套自己的业务规则。

## 不负责什么

不要把这些东西放进 Core：

- Android/Qt/Compose/QML 类型；
- Activity、View、Window、Context；
- IME 协议、候选框、光标绘制；
- 字体塑形、像素坐标、帧时钟、动画时间线；
- 平台权限、通知、生命周期和文件选择器；
- 平台目录猜测、系统密钥库实现、设备环境探测。

需要平台能力时，使用 `writer_platform_api` 已有契约；缺少契约就扩展契约，不在 Core 里判断当前平台。

## 主要入口

- `api/`：稳定 DTO、错误和服务边界。
- `facade` / `app_service`：Core 的业务入口与聚合服务。
- `editor/`：正文事务、revision 和平台无关的编辑/视觉意图。
- `project`、`volume`、`chapter`：作品结构和正文持久化。
- `settings`：设置规则。
- `sync`：同步协议和状态机。
- `starmap`：星图业务数据。
- `writing_stats`：统计。
- `storage`：原子写入。
- `delete_guard`：删除边界。

先修改已有模块；不要为同一业务再建 `*_v2`、`legacy_*`、`fallback_*` 或并行状态机。

## 编辑事务

- 正文持久化始终是纯文本。
- UTF-8 byte offset、range、revision、session generation 在进入编辑逻辑前验证。
- Core 决定正文事务是否成功；动画和渲染结果不能影响事务结果。
- 平台临时状态（IME composing、选区视觉、光标动画）不能写进正文。
- Core 可以返回平台无关的视觉意图，但不计算像素位置和动画时间线。

## 数据与同步

改格式或语义前先看：

- `docs/data_directory_format.md`
- `docs/settings_schema.md`
- `docs/sync_rules.md`
- `docs/starmap_semantics.md`

不要为了 UI 方便直接改变磁盘格式、设置键或同步语义。

删除作品、卷、章节或目录时沿用现有安全删除链路；不要绕过 `delete_guard` 直接拼路径递归删除。

## Rust 约束

- `writer_core` 不依赖平台 crate。
- Rust 内部保持强类型；JSON 只留在确实需要文本协议的边界。
- 外部输入、锁、磁盘和网络错误显式返回，不用 `unwrap`/`expect` 处理服务路径。
- 不手写 `unsafe impl Send/Sync`，不伪造可变引用，不靠资源泄漏解决生命周期。
- 新 `unsafe` 只允许必要 FFI，并写紧邻的 `SAFETY:` 说明。
- 不用宽范围 `allow` 掩盖 warning。

## 常用命令

通用 Core 测试：

```bash
cargo test -p writer_core
```

AI 专项：

```bash
cargo test -p writer_core --features ai --test ai_feature
```

完整 Core Clippy：

```bash
cargo clippy --package writer_core --all-targets --all-features -- -D warnings
```

格式：

```bash
cargo fmt --all --check
```

Rust 安全守卫：

```bash
python3 tools/test_check_rust_safety_patterns.py
python3 tools/check_rust_safety_patterns.py .
```
