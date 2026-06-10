# 仓库架构边界审计表 (Architecture Boundary Audit 2024)

> **当前状态结论**：目前最大的问题是架构边界在实现中被反复打破。Linux UI 页面承担了太多非 UI 的状态流转与持久化职责；Linux 后端又承担了 UI 格式化职责；写作页混用了多套不兼容的布局约束；视觉组件使用不受限。
>
> **最高指令**：在完成审计和修复边界之前，**停止所有功能开发**。不再通过“打补丁”修 bug，而是重新确立领域边界。

## 文件级边界审计明细

| 文件路径 | 当前职责 | 应有职责 | 是否越界 | 越界类型 | 是否允许继续改 | 需要拆到哪里 |
| :--- | :--- | :--- | :---: | :--- | :---: | :--- |
| `apps/desktop/qml/SyncPage.qml` | 既展示页面，又管理同步状态、直接触发并把结果塞回 `syncResultArea`。同一个区域不加区分地接受同步、诊断等多个异步流结果，缺乏操作隔离和状态机。 | 仅限同步配置展示、下发用户交互指令（手动同步、运行诊断）；监听 `ViewModel` 提供的结构化单一事实状态（如 `syncState.currentOperation`）。 | 🚨 是 | 职责混淆、UI 层实现状态机、异步结果覆盖竞争 | 否 | 将状态管理拆至 ViewModel (`SyncController.qml` 或纯状态属性)；异步流需携带 `operation_id` 区分。 |
| `apps/desktop/qml/WritingWorkspace.qml` | 使用 `SplitView` 划分左侧树、中间写作区、右侧抽屉，但中间写作区 `paperBg` 自己用 `MouseArea` 抢夺宽度管理并写入 setting，导致布局约束冲突和 UI 重叠。 | 纯布局容器，严格按单一规则控制三个面板。`SplitView` 负责外层框架约束；编辑器内部自身负责内容展示的最大阅读宽度限制（且只能 `clamp` 在可用宽度内）。 | 🚨 是 | 布局约束重叠、滥用后端 Setting 作为局部状态。 | 否 | 移除中间写作区的手动拖拽宽度（或将其转为单纯的阅读宽度配置），统一由外层 `SplitView` 管理整体可变宽度。 |
| `apps/desktop/src/backend/sync_backend.rs` (及相关) | 负责接收 QML 请求转为 Core 调用，但把异步结果翻译成了大段格式化的中文字符串 `sync_action_result` 和模糊的 `sync_status` 塞回 UI。 | 纯粹的桥接层。负责调用 `core/writer_core` 的接口，并将返回的结构化 `SyncResult` (包含 `status`, `summary`, `details`, `operation_kind`) 原样暴露给前端，而非自己拼接 UI 文案。 | 🚨 是 | 在后端（桥接层）处理 UI 格式化与本地化文案。 | 否 | 停止拼凑字符串。改为向前端返回标准的结构化 JSON 或 QObject 属性集合，让前端自己决定如何展示。 |
| `core/writer_core/src/sync/` (唯一同步模块) | 处理同步算法、接口请求、冲突策略。已合并原 `sync_service`，是唯一正式同步入口。 | 唯一的同步业务真相来源（Single Source of Truth）。负责完整同步逻辑、生成 manifest、冲突判定。返回结果需为纯领域结构。 | ⚠️ 待收敛 | 向前端输出的部分格式化信息需继续收敛为纯语义数据。 | 否 | 清理同步出口，确保只有一个真正的入口，返回严谨的领域级数据结构（如操作日志、改变的文件列表），去格式化。 |
| `apps/desktop/qml/` (全局组件使用) | 全局仍存在大量直接手写 `Text { color: ... }`, `width: ...`, `Rectangle { ... }` 等情况，没有强制使用 `AppText`, `AppButton` 导致深色模式修复不彻底。 | 必须强制组件化。业务页面禁止直接实例化基础图形元素，必须使用白名单系统中的业务/视觉组件（如 `StatusPill`, `AppTextField`）。 | 🚨 是 | 绕过视觉系统 / 组件抽象泄露 | 否 | 对所有裸写 `Text` 和非语义颜色值进行全局替换和封禁。 |
| `tests/` (跨端测试策略) | 当前测试主要在 Core 层进行 `cargo test`。缺乏对 QML UI 状态、同步 Mock 下的行为检查。 | 测试需从“源码级字符串检查”升级为“运行时契约检查”。必须引入 QML smoke tests 和 Fake Backend Mock 测试以覆盖全链路状态流转。 | 🚨 是 | 测试边界不足，UI 行为处于盲区。 | 否 | 需补充 Linux 端的运行时/集成测试。 |

## 下一步行动指南 (Action Items)

1. **结构化同步返回**：重构 `sync_backend.rs`，不再返回 `sync_action_result` 字符串。需定义 `SyncOperationState { operation_id, operation_kind, status, summary, details }` 并向前端提供。
2. **重构写作页布局**：移除 `WritingWorkspace.qml` 中 `paperBg` 的手写拖拽逻辑。使用统一的 `SplitView` (`Layout.fillWidth`, `preferredWidth` 等) 管理三栏布局。
3. **清洗 UI 格式化**：将所有在 Rust 后端硬编码的中文字符串提示、日志拼凑移除，改为向前端返回错误码或参数化数据结构。
4. **组件审计**：清查整个 `qml/` 目录下的 `Text` 节点，强制替换为 `AppText` 等主题组件，彻底解决深色模式断层。
5. **重构同步状态机**：在 `SyncPage.qml` 层面引入唯一状态标识，诊断按钮和同步按钮操作需互斥，且基于 `operation_id` 判断是否更新 UI。
