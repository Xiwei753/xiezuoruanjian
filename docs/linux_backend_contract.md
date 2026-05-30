# Linux Backend Contract

`AppBackend` 是 Linux QML 层的共享状态入口，不是新的业务上帝类。Linux 启动入口的 composition root 是 `BackendRuntime`：它负责创建、持有并注册暴露给 QML 的所有 QObject。

新页面必须按领域使用稳定 backend 名称：`workspaceBackend`、`projectBackend`、`editorBackend`、`settingsBackend`、`syncBackend`、`starmapBackend`。旧 `appBackend.xxx` / `backend.xxx` 调用只允许用于应用级调试、系统主题检测、全局状态兼容；不得把领域 API 重新挂回 `AppBackend`。

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

- `main.rs` 初始化 Qt/QML、创建 `QmlEngine`，然后创建一个长期存活的 `BackendRuntime`。`BackendRuntime` 创建一个 `AppBackend` 共享状态对象，以及六个领域 backend QObject，并通过 `QmlEngine::set_object_property` 注册 `backend` / `appBackend` 以及六个领域 backend context property。
- 六个领域 backend 都是真实 `QObject`，在各自文件中声明 `qt_base_class!`、`qt_property!` 和 `qt_method!`，QML 直接访问领域对象。
- `AppBackend` 的 Qt 宏外观只保留应用级属性/方法与共享信号；领域业务方法继续作为 Rust 内部实现，供领域 QObject 委托调用。
- `apps/linux/src/backend/app_backend/` 下旧 symlink 已删除；该目录只保留普通空 shim 文件，用于绕过 `rust-cpp` 解析器不识别 `#[path = "..."]` 的限制。真实模块仍通过 `#[path = "..."]` 指向 `apps/linux/src/backend/*.rs` 同级领域文件，禁止恢复 symlink 或在 shim 中写业务逻辑。
- 设置弹窗和同步弹窗必须通过 `settingsBackend` / `syncBackend` 名称调用，不再新增全能 backend 路径。

QObject 生命周期规则：

- 暴露给 QML 的 QObject 必须由 Rust 侧稳定 owner 持有，owner 生命周期必须覆盖 `engine.load_file(...)` 和整个 `engine.exec()` event loop。
- `QmlEngine::set_object_property` 只把 QObject 指针放入 QML root context；不得把函数局部临时 `QObjectBox` / `QObjectPinned` 注册给 QML 后让 owner 离开作用域。
- `BackendRuntime` 必须持有 `app_backend`、`workspace_backend`、`project_backend`、`editor_backend`、`settings_backend`、`sync_backend`、`starmap_backend` 的 `QObjectBox` owner，并且必须在 QML load 前完成注册，在 event loop 结束后才释放。
- 已注册给 QML 的 QObject 不得再移动到新的临时容器中，不得只保存裸指针或借用指针。
- QML 中可以保留启动期 null 诊断和极小安全 guard，但禁止用 `Timer`、大面积 fallback 或吞掉 TypeError 的方式掩盖 Rust 侧生命周期错误。
- context property 是当前 qmetaobject 过渡方案。新能力不要无限增加 root context property；后续如条件允许，应优先评估正式 QML module / singleton 路线。
