# AGENTS.md — AI 开发守则

Status: active
Last verified: 2025-07-05
Truth source: code / product decision / protocol
Supersedes: AGENTS.md (previous version)

本文档只记录必须遵守的项目红线。AI 修改代码时按任务需要查阅相关条目；禁止复述仓库结构、目录树、技术栈总览或阅读过程，除非用户明确要求。

---

## 1. 项目架构

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
| 手动修改 UniFFI 自动生成的绑定文件（`writer_core.kt` 等） | 绑定文件由 `uniffi-bindgen` 生成，手动修改会被覆盖；如果生成结果有误，应修 Rust FFI 层根源再重新生成 |

### 2.2 必须遵守

| 要求 | 说明 |
|------|------|
| 修改 Rust core 时写 `cargo test` | 测试覆盖 |
| 新设置项记录到 `docs/settings_schema.md` | 设置 schema 是权威定义 |
| 同步状态、删除确认、错误提示走统一状态/事件通道 | 不允许 QML 直接处理业务状态 |
| QML 只绑定 backend 暴露的 view model / command | 不允许 QML 内部维护业务数据 |
| UniFFI 绑定文件出错时重新生成，不手动修改 | `writer_core.kt` 等由 `uniffi-bindgen` 生成；如果生成结果有误，修 Rust FFI 层根源再重新生成 |

### 2.3 Agent 专属规则与技术路线约束

| 规则与约束 | 说明 |
|-----------|------|
| **唯一技术路线** | `docs/TECHNICAL_ROUTE.md` 是唯一的全局技术路线。 |
| **自研写作区路线** | Linux_qt 自研写作区唯一主路径：`SujianEditorItem(QQuickItem)` + `QTextLayout/QTextLine` + `QImage static texture` + `QSGImageNode` + QML Rectangle cursor + QML `EditorAnimationOverlay`。 |
| **平台路线收口** | Linux_qt/Android 自研写作区已收口（SujianEditorItem + SujianEditorView），不再维护旧 WriterEditText fallback；Harmony 默认 native bridge，不要求 HAP 编译/真机，只要求静态守卫和代码测试。 |

| **正式图谱路线** | 正式图谱是 `starmap`。所有新增图谱能力必须走 StarMapCapability。 |
| **光标修复要求** | 修光标必须先保证 `QTextLine` `xToCursor/cursorToX` roundtrip。 |
| **工作区格式神圣不可侵犯** | 不要仅仅为了迁就 UI 需求而修改 `workspace_format.md` 或改变文件在磁盘上的存储方式。工作区格式是唯一的事实来源。 |
| **保持核心纯净** | 不要将平台特定的 UI 逻辑、动画循环、窗口管理或输入法（IME）处理注入 `writer_core`。核心严格用于数据、逻辑和文件 I/O。 |
| **动画/hidden range 生命周期守卫** | `active_text_animations`、`animatedInsertRange`、光标动画是编辑器内部渲染状态，不是正文数据。

---

## 3. Linux/QML 开发规则

### 3.2 QML 规则

- **避免重复 UI 容器**：一个功能区域只有一层 ScrollView、一个 id。
- **避免魔法 margin**：使用 `dt.sp16`、`dt.sp20` 等 DesignTokens，不要硬编码数字。
- **避免重复滚动层**：TextArea 不要嵌套在多个 ScrollView 中。
- **QML 只绑定数据**：业务逻辑（保存、格式化规则、同步触发）放在 EditorController 或 Rust 侧。
- **编辑区正文永远是纯文本**：`getEditorPlainText()` 用 `getText(0, length)` 取纯文本，替换 `\u2029` 为 `\n`。禁止保存 `targetTextArea.text` 的 HTML 内容。

### 3.3 编辑器排版架构

- Linux_qt 自研写作区当前唯一主路径：`SujianEditorItem(QQuickItem)` + `QTextLayout/QTextLine` + `QImage static texture` + `QSGImageNode` + QML Rectangle cursor + QML `EditorAnimationOverlay`。
- 排查入口：`apps/Linux_qt/src/sujian_editor_item/*`、`apps/Linux_qt/src/editor/layout.rs`、`apps/Linux_qt/src/editor/renderer.rs`、`apps/Linux_qt/src/editor/scene_graph.rs`、`apps/Linux_qt/qml/WritingWorkspace.qml`、`apps/Linux_qt/qml/EditorAnimationOverlay.qml`。
- **禁止**用 `DocumentHandler` / `TextArea` / `QTextDocument` 修自研写作区。这些是旧 fallback 路径，只允许保留兼容，不允许新增依赖；不得作为可开发路线。
- 修自研写作区必须看 `apps/Linux_qt/src/sujian_editor_item/` 和 `docs/editor_engine_route.md`。

### 3.4 openChapter 防死循环

- `openChapter()` 比较 projectId/volumeId/chapterId + `isLoadingChapter` 检查。
- `chapter_path_changed` 信号 **不** 重新触发打开章节。
- 修改加载逻辑时必须验证不会无限刷 `open_chapter_json_start`。

---

## 4. Android 开发规则

- UI 走 Material/现代 Android 组件思路。
- 输入法、写作区、同步设置不能互相污染。
- 不允许硬编码九键候选、拼音路径、词库结果。
- 业务规则和磁盘读写 **必须** 通过 `writer_core` Facade 完成。
- 官方只支持 `arm64-v8a` 构建。

---

## 5. Rust Core 开发规则

### 5.1 规则

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
cd apps/Linux_qt && cargo check && cargo test

# Android
./tools/build_android.sh
```

### 6.2 测试要求

- **能跑单测就跑单测**：不要用全量构建代替。
- **不能跑全量构建时，至少跑相关模块检查**：`cargo check` 比什么都不做强。
- **构建失败必须说明失败原因**：不要假装成功。
- 修改 Rust core 后必须有对应 `cargo test` 覆盖。
- 修改 QML 后验证括号平衡：`python3 -c "..."` 检查 `{` `}` 数量。

---

## 7. Git 规则

- 修改完成后必须推送到 GitHub main。
- 提交信息用中文。
- 不要提交构建产物（target/、build/、*.o、*.so）。
- 不要提交临时日志、测试垃圾文件。
- 不要顺手改不相关的模块（星图、同步、统计、Monet 等）。
- 每次修改聚焦一个明确目标，不要"顺手"改其他东西。

---

## 8. 修改守则

### 8.1 修改前

1. 只读取与当前任务直接相关的文件；不要做通用仓库巡检。
2. QML/Rust 修改前确认局部嵌套结构和括号平衡。
3. 确定修改范围，不要扩散到不相关文件。

### 8.2 修改中

1. **不要删除用户原本的数据结构**：连续空行、段落结构、缩进。
2. **不要引入新的 HTML/富文本保存路径**。
3. **不要为了 UI 需求修改 core 的数据格式**。
4. **不要为了修 UI 在前端堆业务逻辑**。
5. **不要用临时补丁绕过真实的数据流问题**。

### 8.3 修改后

1. **运行最小测试/构建命令**：`cargo check` + `cargo test`（Rust），括号检查（QML）。
2. **推送到 GitHub main**。

---

### 12.3 GitHub 操作必须走 GitHub API

- **查询 GitHub Actions 构建状态、日志、PR、Issue 等信息时，必须使用 `webfetch` 调用 `https://api.github.com/...` REST API**。
- **推送/更新 GitHub 仓库时也必须走 GitHub API + token**：本机的 SSH 和 HTTPS Git 默认端口都被转发/拦截，`git push` 可能卡住或不可靠；不要把 `git push` 当默认推送路径。
- 推荐推送方式：读取项目根目录 `.github-token`，用 GitHub REST API（Contents API 或 Git Database API）更新 `Xiwei753/xiezuoruanjian` 的 `main` 分支。
- **禁止使用 `gh` CLI 命令**：本机未安装 GitHub CLI（`gh.exe`），PATH 中的 `gh` 是一个无关的 Python 脚本，无法执行 GitHub 操作。
- 本项目仓库：`Xiwei753/xiezuoruanjian`
- 认证 token 在项目根目录 `.github-token` 文件（已加入 `.gitignore`，不会提交）。
- **绝对不要向用户索要 token。** 需要认证时直接读取 `.github-token` 文件，不要问用户要。
- 不要输出、提交或记录 token 原文。

#### 推送命令（必须用此方式，禁止 `git push`）

项目根目录已有 `tools/api_push.py`，封装了 GitHub Git Database API 推送逻辑。**每次需要推送到 GitHub 时，必须使用以下命令：**

```bash
# 先本地 commit，然后：
python tools/api_push.py "<token>" Xiwei753/xiezuoruanjian main
```

其中 `<token>` 从 `.github-token` 文件读取。完整流程：

1. `git add <files>`
2. `git commit -m "..."`
3. 读取 `.github-token` 文件获取 token
4. `python tools/api_push.py "<token>" Xiwei753/xiezuoruanjian main`

**绝对不要用 `git push`**——本机网络环境会导致卡死或超时。

#### GitHub Actions 日志查询流程

1. **获取 workflow 列表**：`webfetch` → `https://api.github.com/repos/Xiwei753/xiezuoruanjian/actions/workflows`
2. **获取某 workflow 的运行列表**：`webfetch` → `https://api.github.com/repos/Xiwei753/xiezuoruanjian/actions/workflows/{workflow_id}/runs?per_page=5`
3. **获取某次运行的 jobs**：`webfetch` → `https://api.github.com/repos/Xiwei753/xiezuoruanjian/actions/runs/{run_id}/jobs`
4. **获取 check-run annotations**：`webfetch` → `https://api.github.com/repos/Xiwei753/xiezuoruanjian/check-runs/{check_run_id}/annotations`
5. **下载日志**（需要认证）：
   - 日志 API：`https://api.github.com/repos/Xiwei753/xiezuoruanjian/actions/jobs/{job_id}/logs`
   - `webfetch` 不支持自定义 Authorization header，且 URL 嵌入 token 的方式 (`x-access-token:*** 也被 GitHub 拒绝（403）。
   - **如果 PAT 没有 `actions:read` 权限，无法通过 API 下载日志**，此时需要用户手动从 GitHub 网页复制日志内容。
   - 替代方案：在 CI workflow 中将关键输出写入 `$GITHUB_STEP_SUMMARY`，这样可以通过 check-run annotations 或 job 步骤结果间接获取。

### 12.4 本地依赖

- Rust 工具链：`cargo`、`rustc` 已在 PATH 中。
- Qt6：本地开发环境需自行安装，Linux 二进制不应链接 Qt5。
- Android：仅支持 `arm64-v8a` 构建。

---
