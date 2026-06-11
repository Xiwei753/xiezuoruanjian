# AGENTS.md — AI 开发守则

Status: active
Last verified: 2026-06-11
Truth source: code / product decision / protocol
Supersedes: `legacy ai development guide`, `legacy ai tool calling doc`, AGENTS.md (previous version)

本文档是给 AI 助手看的项目规则。修改代码前必须通读。

---

## 1. 项目架构

```
core/writer_core/          Rust 核心库（唯一业务逻辑层）
bindings/android/          Android JNI 桥接
bindings/shared/           跨平台共享绑定
apps/android/              Kotlin Android 客户端（薄客户端）
apps/desktop/                Qt/QML Linux 客户端（薄客户端）
```

**核心原则：Rust Core 是唯一事实来源。**

- 所有文件 I/O、项目管理、同步、格式化、设置规则 **必须** 在 `core/writer_core` 中实现。
- 客户端只负责 UI 渲染、导航、输入法交互和主题。
- 客户端 **不允许** 自行拼接路径、处理 Workspace 规则、实现业务分支。

---

## 2. 架构红线

### 2.1 绝对禁止

| 禁止行为 | 原因 |
|---------|------|
| 在 QML/Qt 中写复杂 JSON 解析和业务分支 | 前端只做展示和状态绑定 |
| 在 Android 中硬编码九键候选、拼音路径、词库结果 | 输入法逻辑下沉到 core 或 binding |
| 为了修 UI 在前端堆业务逻辑 | 违反薄客户端原则 |
| 修改 `workspace_format.md` 来迁就 UI | 工作区格式是唯一事实来源，不可为 UI 妥协 |
| 在 Rust core 中注入平台特定 UI 逻辑、动画、窗口管理 | core 严格排除 UI |
| 输出用户稿件内容、API 密钥到应用日志 | 数据隐私 |
| 为了首行缩进往正文里插入空格 | 破坏纯文本 |
| 把正文保存成 HTML | 正文永远是纯文本 |
| 恢复旧的 HTML 字符串拼接排版 | 已废弃的方案 |

### 2.2 必须遵守

| 要求 | 说明 |
|------|------|
| 修改 Rust core 时写 `cargo test` | 测试覆盖 |
| 新设置项记录到 `docs/settings_schema.md` | 设置 schema 是权威定义 |
| 同步状态、删除确认、错误提示走统一状态/事件通道 | 不允许 QML 直接处理业务状态 |
| QML 只绑定 backend 暴露的 view model / command | 不允许 QML 内部维护业务数据 |

### 2.3 Agent 专属规则与技术路线约束

| 规则与约束 | 说明 |
|-----------|------|
| **唯一技术路线** | `docs/TECHNICAL_ROUTE.md` 是唯一的全局技术路线。 |
| **自研写作区路线** | Desktop 自研写作区当前路线是 `SujianEditorItem + QTextLayout`。 |
| **禁止旧路线排版修复** | **禁止**再按 `QTextDocument` / `DocumentHandler` 旧排版路线修自研写作区。 |
| **正式图谱路线** | `mind_map` 是 legacy（已废弃），正式图谱是 `starmap`。所有新增图谱能力必须走 StarMapCapability。 |
| **淘汰 envelope_json** | `envelope_json` 是 legacy 兼容，新功能**绝对禁止**使用，必须完全采用 typed DTO。 |
| **光标修复要求** | 修光标必须先保证 `QTextLine` `xToCursor/cursorToX` roundtrip。 |
| **工作区格式神圣不可侵犯** | 不要仅仅为了迁就 UI 需求而修改 `workspace_format.md` 或改变文件在磁盘上的存储方式。工作区格式是唯一的事实来源。 |
| **保持核心纯净** | 不要将平台特定的 UI 逻辑、动画循环、窗口管理或输入法（IME）处理注入 `writer_core`。核心严格用于数据、逻辑和文件 I/O。 |

---

## 3. Linux/QML 开发规则

### 3.1 目录结构

```
apps/desktop/
  src/
    main.rs              入口 + AppBackend QObject（业务绑定层）
    document_handler.rs  QTextDocument 排版操作（独立 QObject）
    starmap_bridge.rs    星图桥接
    sync_bridge.rs       同步桥接
  qml/
    main.qml             应用入口
    WritingWorkspace.qml 写作工作区（编辑区 + 侧栏）
    EditorController.qml 编辑器逻辑控制器
    TopWritingToolbar.qml 写作工具栏
    StarMapPage.qml      星图页面
    SyncPage.qml         同步页面
    SettingsDialog.qml   设置对话框
    ...其他 QML 组件
```

### 3.2 QML 规则

- **避免重复 UI 容器**：一个功能区域只有一层 ScrollView、一个 id。
- **避免魔法 margin**：使用 `dt.sp16`、`dt.sp20` 等 DesignTokens，不要硬编码数字。
- **避免重复滚动层**：TextArea 不要嵌套在多个 ScrollView 中。
- **QML 只绑定数据**：业务逻辑（保存、格式化规则、同步触发）放在 EditorController 或 Rust 侧。
- **编辑区正文永远是纯文本**：`getEditorPlainText()` 用 `getText(0, length)` 取纯文本，替换 `\u2029` 为 `\n`。禁止保存 `targetTextArea.text` 的 HTML 内容。

### 3.3 编辑器排版架构

- Desktop 自研写作区当前主路径是 SujianEditorItem + QTextLayout。
- DocumentHandler 只作为 legacy/stable TextArea 兼容辅助，不得用于修复自研写作区光标、命中、滚动、动画。
- 修自研写作区必须只看 apps/desktop/src/sujian_editor_item/mod.rs 和 docs/editor_engine_route.md。

### 3.4 openChapter 防死循环

- `openChapter()` 比较 projectId/volumeId/chapterId + `isLoadingChapter` 检查。
- `chapter_path_changed` 信号 **不** 重新触发打开章节。
- 修改加载逻辑时必须验证不会无限刷 `open_chapter_json_start`。

### 3.5 常见陷阱

| 陷阱 | 正确做法 |
|------|---------|
| 正文最大宽度未限制，宽屏下每行无限拉长 | ScrollView 锚定 paperBg（max-width 820px），设置 `contentWidth: availableWidth` |
| 一键排版压缩连续空行 | 只清理段首缩进空格和行尾空格，保留空行 |
| 保存时误存 HTML | 保存前调用 `sanitizePlainText()` 检测并剥离 HTML 标签 |
| 切换字号/行距触发重新打开章节 | 通过 property binding 自动更新，不触发 save/reload |
| autosave 时传入 HTML | 统一走 `saveCurrentChapter()` → `getEditorPlainText()` → `sanitizePlainText()` |

---

## 4. Android 开发规则

- UI 走 Material/现代 Android 组件思路。
- 输入法、写作区、同步设置不能互相污染。
- 不允许硬编码九键候选、拼音路径、词库结果。
- 业务规则和磁盘读写 **必须** 通过 `writer_core` Facade 完成。
- 官方只支持 `arm64-v8a` 构建。

---

## 5. Rust Core 开发规则

### 5.1 目录结构

```
core/writer_core/
  src/
    facade.rs            Facade 层（对外 API 入口）
    workspace/           工作区管理
    project/             项目管理
    sync/                同步逻辑
    settings/            设置管理
    formatting/          格式化规则
    ...
```

### 5.2 规则

- `core/writer_core` 是唯一业务底层核心库。
- 处理所有文件 I/O、项目管理、同步、格式化和设置规则。
- **严格排除 UI 逻辑**（动画、窗口管理、输入法、平台特定代码）。
- 修改 core 后必须跑 `cargo test`。

---

## 6. 测试与验证

### 6.1 测试命令

```bash
# Rust 核心
cd core/writer_core && cargo test

# Linux 客户端
cd apps/desktop && cargo check && cargo test

# Android
./tools/build_android.sh
```

### 6.2 测试要求

- **能跑单测就跑单测**：不要用全量构建代替。
- **不能跑全量构建时，至少跑相关模块检查**：`cargo check` 比什么都不做强。
- **构建失败必须说明失败原因**：不要假装成功。
- 修改 Rust core 后必须有对应 `cargo test` 覆盖。
- 修改 QML 后验证括号平衡：`python3 -c "..."` 检查 `{` `}` 数量。

### 6.3 实机验证步骤

1. 新建作品。
2. 进入作品，确认自动出现"第一卷"。
3. 点击新建章节，输入内容。
4. 确认自动保存。
5. 退出后重新进入该章节，确认内容仍在。
6. 调整字号/行距/首行缩进，确认正文文件内容不变。
7. 宽屏下确认正文居中且最大宽度稳定。
8. 一键排版确认不吞连续空行。

---

## 7. Git 规则

- 修改完成后必须推送到 GitHub main（除非用户明确禁止）。
- 提交信息用中文，说明改了哪些文件和为什么。
- 不要提交构建产物（target/、build/、*.o、*.so）。
- 不要提交临时日志、测试垃圾文件。
- 不要顺手改不相关的模块（星图、同步、统计、Monet 等）。
- 每次修改聚焦一个明确目标，不要"顺手"改其他东西。

---

## 8. 修改守则

### 8.1 修改前

1. **读取目标文件的当前状态**，不要凭记忆假设。
2. **理解文件的嵌套结构 and 括号平衡**，QML/Rust 都是。
3. **确定修改范围**，不要扩散到不相关的文件。

### 8.2 修改中

1. **不要删除用户原本的数据结构**：连续空行、段落结构、缩进。
2. **不要引入新的 HTML/富文本保存路径**。
3. **不要为了 UI 需求修改 core 的数据格式**。
4. **不要为了修 UI 在前端堆业务逻辑**。
5. **不要用临时补丁绕过真实的数据流问题**。

### 8.3 修改后

1. **运行最小测试/构建命令**：`cargo check` + `cargo test`（Rust），括号检查（QML）。
2. **说明改了哪些文件**。
3. **推送到 GitHub main**。

---

## 9. 文件权限矩阵

| 文件/目录 | 允许修改 | 禁止修改 |
|-----------|---------|---------|
| `core/writer_core/` | Rust 核心逻辑 | — |
| `apps/desktop/src/main.rs` | AppBackend 绑定 | 不要新增写作排版细节 |
| `apps/desktop/src/document_handler.rs` | QTextDocument 操作 | 不要搬回 main.rs |
| `apps/desktop/qml/*.qml` | UI 组件和状态绑定 | 不要写复杂业务逻辑 |
| `apps/android/` | Android 客户端 | 不要硬编码输入法逻辑 |
| `.github/` | CI/CD 配置 | — |
| `docs/*.md` | 文档 | — |
| `workspace_format.md` | — | **绝对禁止修改** |

---

## 10. 一句话总结

> **Rust Core 管逻辑，客户端管展示，QML 只绑定不计算，正文永远是纯文本。**

---

## 11. DeepSeek Thinking Mode + Tool Calls 注意事项

由于 DeepSeek API 在使用 Thinking Mode（深度思考模式）时，对工具调用（Tool Calls）有特殊要求，本项目在底层做做出特定的兼容处理：

- **tool_calls 场景下 reasoning_content 必须完整回传**：当 assistant 产生工具调用请求时，其返回的 `reasoning_content` 必须在随后的请求中原样附带回传，否则会触发 DeepSeek API 返回 400 错误。
- **普通无工具调用对话不应该把旧 reasoning_content 带回**：如果 assistant 没有产生任何工具调用，而是直接输出了回复结果，此时之前的 `reasoning_content` 已经完成使命，在未来的多轮对话上下文中会自动剔除（丢弃），不再回传以节省 tokens 并且避免造成上下文污染。
- 本项目通过 **DeepSeekMessageSerializer** 和 **AIConversationSession** 实现针对 provider 特殊行为的隔离与适配：
  - `DeepSeekMessageSerializer` 负责根据 assistant 是否存在 `tool_calls` 以及提供商是否为 DeepSeek 来动态决定是否序列化保留 `reasoning_content`。
  - `AIConversationSession` 用于记录会话上下文，如果触发工具调用，它标记 `requiresReasoningContentEcho = true` 来帮助系统进行流转控制。
- `reasoning_content` 是属于提供商在生成结果过程中的隐藏计算过程数据：
  - **不展示给用户**（UI 不应把它当成普通的 `content` 处理）。
  - **不写入正文**（它不是用户的写作数据，不可进入 `chapter.md` 等文档）。
  - **不写入日志**（考虑可能会占用较多内存和控制台空间）。
  - 需要持久化记录时只能进入 `app-meta/ai/traces/` 目录下的跟踪文件。
