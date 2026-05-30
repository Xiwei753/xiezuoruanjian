# Linux Backend Contract

`AppBackend` 是 Linux QML 层的 composition root 和兼容入口，不是新的业务上帝类。

新页面必须按领域使用稳定 backend 名称：`workspaceBackend`、`projectBackend`、`editorBackend`、`settingsBackend`、`syncBackend`、`starmapBackend`。旧 `appBackend.xxx` / `backend.xxx` 调用只作为 deprecated compatibility forwarding 保留，等 QML 迁移完成后删除。

职责边界：

- `WorkspaceBackend`：工作区恢复、打开、切换、关闭、GitHub 初始化、工作区路径与诊断。
- `ProjectBackend`：作品、卷、章节树结构的创建、删除、重命名、排序、选择和应用状态刷新。
- `EditorBackend`：章节打开、正文保存、编辑器状态、字数统计、写作统计和 Action 执行。
- `SettingsBackend`：字号、行距、主题、Monet 色、动画、自动保存、自动缩进、AI 开关和本地设置持久化。
- `SyncBackend`：同步配置、Token 状态、诊断、手动同步、自动同步触发、同步日志和同步状态。
- `StarMapBackend`：StarMap 列表、图加载、节点/边操作、布局保存，以及旧 mind map JSON 入口。

禁止事项：

- 禁止把所有新方法重新堆回 `AppBackend`。
- 禁止在 QML 中新增复杂 JSON 解析或业务分支。
- 禁止为了 UI 迁移改变 JSON schema、同步协议或 Rust Core 同步算法。
- 禁止让子 backend 文件退化成只含注释的空壳。

当前兼容策略：

- `AppBackend` 仍保留旧 Qt 暴露面，避免一次性删除旧 QML 调用导致页面炸裂。
- 领域方法代码已按文件拆出，`app_backend.rs` 只保留全局初始化、共享状态字段、调试/平台工具和 deprecated 兼容聚合。
- 设置弹窗和同步弹窗必须通过 `settingsBackend` / `syncBackend` 名称调用，不再新增全能 backend 路径。
