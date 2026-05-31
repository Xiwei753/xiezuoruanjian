# Linux Backend QObject 桥接模块

本目录包含所有的 QML 对接 Rust 后端。它们完全被设计为**薄后端适配层 (Thin Adapter Layers)**。

## 领域 QObject 设计

根据项目的 API 收口与独立领域划分，后台业务被拆分为以下专职 QObject 对象：

- **WorkspaceBackend** (`workspace_backend.rs`)：负责暴露当前工作区状态、主侧栏树型目录的数据源提供。
- **ProjectBackend** (`project_backend.rs`)：负责新建项目、分卷等非频繁保存类的管理动作。
- **EditorBackend** (`editor_backend.rs`)：负责编辑区状态展示，将纯文本通过 QTextDocument 进行传递。
- **SettingsBackend** (`settings_backend.rs`)：负责在界面层消费强类型的 settings schema，控制外观和字体参数。
- **SyncBackend** (`sync_backend.rs`)：**高危异步模块**。
  - 核心实现了基于 `operation_id` 标志的异步结果验证。
  - 通过 `perform_sync` / `perform_sync_diagnostics` 返回生成的唯一 UUID。
  - 在异步回调中校验结果的 operation_id，丢弃过期或竞争性异步事件，杜绝多任务同时触发时对界面同步输出区的竞态覆盖。
- **StarMapBackend** (`starmap_backend.rs`)：星图关系的桥接层。

## 架构红线

- 后端各文件**绝对禁止**进行复杂的 UI 日志文字拼接，异步结果一律格式化为标准 JSON (如 `SyncOperationState`) 塞回 UI，由前端自行本地化渲染。
