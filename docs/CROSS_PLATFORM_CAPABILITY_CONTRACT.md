# 《跨平台能力契约与 Core-first 架构约束》

> [!NOTE]
> **进度追踪**：请查看 [跨平台能力矩阵 (CAPABILITY MATRIX)](CAPABILITY_MATRIX.md) 了解当前各能力在 Core / Android / Desktop 三边的分叉状态及下一步重构顺序。

> [!IMPORTANT]
> **最高优先级规则**
> - 本文档约束所有跨平台业务能力。
> - 后续任何功能，如果涉及 Workspace / Project / Volume / Chapter / Settings / Sync / StarMap / Editor Model / Export / AI Context / Search / Trash / Delete Guard / Conflict Handling，都必须先检查本文档。Mind Map 已废弃（仅迁移兼容），新功能禁止使用。
> - 如果提示词、AI 任务、人工 PR 与本文档冲突，以本文档为准。
> - 如需改变本文档，必须单独提交文档变更并说明原因，不能在功能代码里绕开。

---

## 一、 核心原则

### 1. Core-first (核心优先)
- **业务真相在 Core**：所有业务能力、业务状态转移、规则校验必须先在 Rust Core (`core/writer_core`) 中定义语义和实现。
- **平台层只读/委托**：平台端（Android / Desktop 及未来的其他端）只能调用 Core capability，禁止在平台端自行编写、复制或发明长期业务规则或数据校验。
- **职责边界**：平台端可以负责 UI 呈现、输入交互、系统权限申请、底层渲染、原生生命周期生命线和系统通知，但绝对不能成为任何业务数据的“真相来源”。

### 2. One Capability, Many Bindings (单一能力，多端绑定)
- **语义归一**：一个业务能力（例如：创建作品、修改设置）在 Core 中必须有且仅有一个业务语义定义。
- **无歧义绑定**：Android UniFFI 主绑定、legacy JNI fallback、Desktop backend 适配器、以及未来的 Windows/macOS/iOS/Web 绑定层，都只能作为该 Capability 的 Binding。
- **传输层格式隔离**：Binding 可以为了平台特性使用不同的传输格式（如 Android UniFFI typed DTO、legacy JSON 字符串/DirectByteBuffer，Desktop qmetaobject 传输 C++ 结构体/JSON），但传输格式绝不能修改、扩展或削弱 Core API 的原始业务语义。
- **反例**：Android 端实现 `createProject` 包含自动初始化空卷的业务语义 A，而 Desktop 端实现的 `createProject` 只是创建空文件夹的业务语义 B。这种情况被绝对禁止。

### 3. Shared Result Envelope (统一结果信封)
- **标准响应结构**：所有跨平台 Capability 调用必须统一返回结果包裹格式。禁止直接返回裸状态值或各平台自定义的响应体。
- **信封字段定义**：
  ```json
  {
    "success": true, // 是否成功执行
    "data": {}, // 业务返回数据 payload，可能为空
    "errorCode": "ERROR_CODE_STRING", // 统一的标准大写下划线错误码，如 "WORKSPACE_LOCKED"
    "userMessage": "友好的用户提示信息", // 默认的可翻译中文/英文描述
    "rawError": "libgit2 error details...", // 平台底层或依赖库暴露的原始错误详情，用于日志定位
    "warnings": [], // 非致命的警告信息列表
    "changedPaths": [], // 发生变更的文件绝对或相对路径
    "changedEntities": [] // 发生变更的实体类型和 ID 列表，便于平台通知 UI 刷新
  }
  ```
- **错误推断规则**：平台层不得通过解析 `rawError` 中的字符串或通过黑盒表现来猜测错误类型。所有的错误分支必须由 Core 通过 `errorCode` 和 `userMessage` 显式抛出。
- **敏感数据脱敏**：任何涉及同步 token、secrets、账户密码的信息，在 Core 返回的 Result Envelope 中必须强制脱敏，只允许以掩码或布尔状态暴露。

### 4. Shared Status Model (统一状态模型)
- 对同步、设置保存、导图写入、作品创建/删除等高危或复杂长周期操作，其运行期状态必须由 Core 统一定义，并输出以下标准状态机状态，平台层直接映射，不得二次包装或自己发明新状态：
  - `idle`：空闲状态。
  - `running`：正在执行中。
  - `success`：执行成功。
  - `failed`：执行失败。
  - `conflict`：同步/并发冲突。
  - `validation_error`：数据校验失败。
  - `auth_error`：身份验证失败（Token 失效、未登录等）。
  - `network_error`：网络连接失败。
  - `not_found`：请求的文件/实体不存在。
  - `permission_denied`：权限不足或工作区锁保护。

### 5. Shared Command Model (统一写操作命令模型)
- 任何对 Workspace / Project / Volume / Chapter / MindMap / Settings / Sync 的写入或变更操作，必须抽象为 Core command。
- 每个 Command 必须在 Core 中独立且完整地定义：
  - **Request Schema**：输入参数类型及合法性校验规则（`validation`）。
  - **Mutation Logic**：核心文件/内存修改逻辑，且该过程必须为原子操作或支持 Crash-safe 回滚。
  - **Response Schema**：输出的数据包，指出受影响的实体（`changedEntities`）。
  - **Error Mapping**：对底层 IO/Git/序列化错误的统一捕获和 errorCode 映射。
  - **Core Tests**：必须包含针对此 Command 的独立单元测试和集成测试，确保脱离 UI 时逻辑 100% 正确。
- **禁止平台绕道**：平台端严禁越过 Command 机制直接修改 workspace 下的任何文件（包括但不限于 `mind_map/index.json`、`settings.json`、`.git/` 下的文件）。

### 6. Shared Event Model (统一事件模型)
- 业务事件（Domain Event）由 Core 统一捕捉并定义，以用于跨平台通信与 UI 异步刷新：
  - `ProjectCreated` (项目创建)
  - `ProjectDeleted` (项目删除)
  - `ChapterUpdated` (章节内容/元数据修改)
  - `SettingsSaved` (本地或同步设置已保存)
  - `SyncConflictDetected` (同步检测到不可自动合并的冲突)
  - `MindMapGraphUpdated` (思维导图图谱/布局发生变更)
  - `AnchorBroken` (正文变动导致思维导图的锚点失效)
- **消费规则**：平台端仅能作为 Event 的消费者。平台端可以把事件转换为特定平台的 UI State 或 LiveData/StateFlow 信号，但禁止自己在平台层发明和分发长周期的业务事件。

### 7. Platform Adapter Only (平台绑定层纯适配器化)
- Android 中的 `AppServiceBridge + UniFFI` 主链路、`NativeCoreBridge`/`writer_core_jni` legacy fallback，以及 Desktop 中的 `qmetaobject/backend` 只是**适配器（Adapter）**，不是业务逻辑实现层。
- **适配器允许且只允许做以下事情**：
  1. 数据格式的物理转换（如：Rust RustString -> C++ QString，Rust Vector -> Kotlin List）。
  2. 调度执行：将同步、写操作派发到非 UI 线程，或接收 Core 异步回调并派发到平台主线程。
  3. 生命周期绑定：在 Activity 或 Window 销毁时停止尚未完成的轮询或取消监听。
  4. UI 翻译：将 Core 统一传出的 `errorCode`/`userMessage` 转化为各平台原生的 Dialog / Toast / HUD 提醒。
- **适配器禁止事项**：
  1. 禁止自行判断操作是否成功（如：判定文件存在就认为 createProject 成功）。
  2. 禁止自行修改 workspace 里的任何长期文件。
  3. 禁止静默吞掉 Core 传出的错误。
  4. 禁止在 Core 返回失败时制造虚假成功状态以试图“平滑体验”。
  5. 禁止针对 Android 和 Desktop 实现两套截然不同的业务规则。

---

## 二、 目录职责划分

```mermaid
graph TD
    subgraph Core ["core/writer_core (Rust Core)"]
        CAP[Capability API] --> DATA[数据模型 & 结构]
        CAP --> VALID[写操作校验 Command]
        CAP --> EVENT[事件系统 Event]
        CAP --> TEST[业务核心测试]
    end

    subgraph Bindings ["bindings (薄绑定层)"]
        UNI[UniFFI generated binding]
        JNI[bindings/android (legacy JNI Adapter)]
        SO[bindings/shared (C-ABI Shared)]
    end

    subgraph AndroidApp ["apps/android (Android 壳应用)"]
        AVM[ViewModel & LiveData] --> AUI[UI / Activity / Custom Canvas View]
        AVM --> DomainBridge[Domain Bridges]
        DomainBridge --> AppServiceBridge[AppServiceBridge]
        AVM -. legacy status/action .-> NCBridge[NativeCoreBridge]
    end

    subgraph DesktopApp ["apps/desktop (Desktop 壳应用)"]
        LBACK[Backend Adapter C++] --> LUI[QML UI]
    end

    AppServiceBridge --> UNI
    UNI --> CAP
    NCBridge --> JNI
    JNI --> CAP
    LBACK --> SO
    SO --> CAP
```

- **`core/writer_core` (唯一核心业务层)**
  - Capability API 的定义与实现。
  - 标准数据结构与文件系统布局。
  - 同步白名单、版本兼容、合并与冲突解析策略。
  - 核心错误码与标准状态码。
  - 覆盖所有业务逻辑的集成/单元测试。

- **Android UniFFI / legacy JNI 物理桥接**
  - 主业务使用 UniFFI 生成绑定；legacy JNI 只保留 fallback。
  - 必须足够轻薄，主要做数据类型映射与转换。
  - 不引入 Kotlin/Java 的额外业务判断。
  - 不包含任何直接对本地 workspace 的文件读写操作。

- **`apps/android` (Android 客户端壳层)**
  - UI 渲染、组件渲染、Activity/Fragment 生命周期的处理。
  - 通过领域 Bridge 调用 `AppServiceBridge + UniFFI`；只有 legacy 状态/动作路径可以调用 `NativeCoreBridge`。
  - 不做本地业务缓存。在设置界面编辑时，只允许在内存中修改 Draft，直到点击“保存”通过 SettingsCapability 写入，以 Core 返回的 `SettingsSaved` 事件为刷新 UI 的唯一依据。
  - 导图的拖拽缩放、手势事件、惯性计算是 UI 逻辑，但节点布局、数据和锚点更新全量来自 Core。

- **`apps/desktop` (Desktop 桌面客户端壳层)**
  - 基于 Qt/QML 的 UI 部分和 C++ 编写的 Desktop backend 适配层。
  - QML 只绑定 AppState 里的只读变量，并向后端发送用户指令动作。
  - 禁止在 QML 里使用 `Timer` 去间接轮询判断某项后台业务是否成功。
  - 禁止在 QML 里编写任何修改业务状态的状态机逻辑。

---

## 三、 Capability API 清单

为了彻底杜绝 Android 和 Linux 在双端各自为战，以下业务模块已定义为统一的 Capability API，双端后续实现时必须直接对照此清单进行对齐和封装：

### 1. WorkspaceCapability
- `createWorkspace(path: String, name: String) -> ResultEnvelope`
- `openWorkspace(path: String) -> ResultEnvelope`
- `validateWorkspace(path: String) -> ResultEnvelope` (工作区结构与合法性校验)
- `getWorkspaceState() -> WorkspaceState` (锁状态、同步配置、最近使用等)

### 2. ProjectCapability
- `createProject(workspacePath: String, name: String) -> ResultEnvelope`
- `renameProject(workspacePath: String, projectId: String, newName: String) -> ResultEnvelope`
- `deleteProject(workspacePath: String, projectId: String) -> ResultEnvelope`
- `listProjects(workspacePath: String) -> ResultEnvelope<List<Project>>`
- `getProjectTree(workspacePath: String, projectId: String) -> ResultEnvelope<ProjectTree>`

### 3. ChapterCapability
- `createVolume(workspacePath: String, projectId: String, volumeName: String) -> ResultEnvelope`
- `createChapter(workspacePath: String, projectId: String, volumeId: String, chapterName: String) -> ResultEnvelope`
- `renameChapter(workspacePath: String, projectId: String, chapterId: String, newName: String) -> ResultEnvelope`
- `deleteChapter(workspacePath: String, projectId: String, chapterId: String) -> ResultEnvelope`
- `loadChapter(workspacePath: String, projectId: String, chapterId: String) -> ResultEnvelope<ChapterData>`
- `saveChapter(workspacePath: String, projectId: String, chapterId: String, content: String) -> ResultEnvelope`
- `getStats(workspacePath: String, projectId: String, chapterId: String) -> ResultEnvelope<ChapterStats>`

### 4. SettingsCapability
- `getLocalSettings(workspacePath: String) -> ResultEnvelope<LocalSettings>`
- `saveLocalSettings(workspacePath: String, settings: LocalSettings) -> ResultEnvelope`
- `getSyncableSettings(workspacePath: String) -> ResultEnvelope<SyncableSettings>`
- `saveSyncableSettings(workspacePath: String, settings: SyncableSettings) -> ResultEnvelope`
- `getEffectiveSettings(workspacePath: String) -> ResultEnvelope<EffectiveSettings>` (合并本地与同步配置的最终计算设置)
- **触发事件**：在写入成功时强制发出 `SettingsSaved` 广播事件。

### 5. SyncCapability
- `loadSyncConfig(workspacePath: String) -> ResultEnvelope<SyncConfig>`
- `saveSyncConfig(workspacePath: String, config: SyncConfig) -> ResultEnvelope`
- `dryRun(workspacePath: String) -> ResultEnvelope<SyncReport>` (演练，返回受影响文件)
- `diagnostics(workspacePath: String) -> ResultEnvelope<SyncDiagnostics>` (诊断本地库状态)
- `sync(workspacePath: String) -> ResultEnvelope<SyncResult>` (执行推送与拉取)
- **映射规则**：底层调用如 `libgit2` 产生的错误，必须在 Core 中全部映射为 `SyncStatus`，严禁直接把 libgit2 裸字符串传递给平台层展示。
- **冲突处理**：当发生 Merge 冲突时，冲突判定、备选方案选择（保留本地/覆盖云端/人工合并）必须由 Core 定义的冲突处理算法进行，平台只提供选择 UI。

### 6. MindMapCapability (LEGACY - 已废弃)

> **⚠️ 本 Capability 已废弃，仅保留用于旧数据迁移兼容。**
> 正式图谱路线为 `starmap`（星图）。所有新增图谱能力必须走 StarMapCapability，禁止继续在 MindMapCapability 中开发新功能。

- `createGraph(workspacePath: String, projectId: String, graphName: String) -> ResultEnvelope<GraphId>`
- `listGraphs(workspacePath: String, projectId: String) -> ResultEnvelope<List<GraphMeta>>`
- `setDefaultGraph(workspacePath: String, projectId: String, graphId: String) -> ResultEnvelope`
- `createNode(workspacePath: String, projectId: String, graphId: String, parentId: String, nodeText: String) -> ResultEnvelope<NodeId>`
- `updateNode(workspacePath: String, projectId: String, graphId: String, nodeId: String, newText: String) -> ResultEnvelope`
- `deleteNode(workspacePath: String, projectId: String, graphId: String, nodeId: String) -> ResultEnvelope`
- `createEdge(workspacePath: String, projectId: String, graphId: String, sourceId: String, targetId: String) -> ResultEnvelope<EdgeId>`
- `updateEdge(workspacePath: String, projectId: String, graphId: String, edgeId: String, properties: EdgeProperties) -> ResultEnvelope`
- `deleteEdge(workspacePath: String, projectId: String, graphId: String, edgeId: String) -> ResultEnvelope`
- `createAnchor(workspacePath: String, projectId: String, graphId: String, nodeId: String, chapterId: String, anchorOffset: Int, anchorText: String) -> ResultEnvelope<AnchorId>`
- `resolveAnchor(workspacePath: String, projectId: String, graphId: String, anchorId: String) -> ResultEnvelope<ResolvedAnchor>`
- `bindAnchor(workspacePath: String, projectId: String, graphId: String, anchorId: String, nodeId: String) -> ResultEnvelope`
- `saveLayout(workspacePath: String, projectId: String, graphId: String, layoutData: LayoutData) -> ResultEnvelope`
- `getSnapshot(workspacePath: String, projectId: String, graphId: String) -> ResultEnvelope<MindMapSnapshot>` (只读渲染视图快照)

### 6b. StarMapCapability (正式图谱路线)
- StarMap 是唯一推荐的图谱能力入口，详见 `starmap` 模块和 `starmap_semantics.md`。
- 提供星图元数据管理、图数据 CRUD、布局持久化、viewport 状态、embed/link 语义边、连线渲染和命中测试等能力。

### 7. EditorModelCapability
- `loadChapterText(workspacePath: String, projectId: String, chapterId: String) -> ResultEnvelope<EditorTextState>`
- `saveChapterText(workspacePath: String, projectId: String, chapterId: String, transaction: TextTransaction) -> ResultEnvelope`
- `computeWordStats(text: String) -> WordStats`
- `trackSessionStats(workspacePath: String, charsAdded: Int, durationSeconds: Int) -> ResultEnvelope`
- **排版控制**：自动缩进（autoIndent）等格式化行为必须读取 Core 中 Settings 对应的配置，平台端排版和编辑器渲染器只消费状态。

### 8. 未来 AI Capability (AI 业务功能)
- **安全隔离**：AI 辅助功能（如扩写、续写、提示节点）绝对不允许直接操纵或写入平台侧的局部状态。
- **动作化**：AI 生成的内容在被用户采纳前，只作为 `Core Action Proposal` 传输给平台；用户点击接受后，必须作为 Core Command 写入 workspace 历史中。
- **图谱输入**：AI 自动抽取的节点、大纲、逻辑锚点，必须走 `MindMapCapability` 提供的标准接口导入。

---

## 四、 具体对齐实例与反例

### 实例 1：创建项目 (createProject)
- ❌ **错误做法**：
  Android 通过 JNI 调起 `createFolder`，然后在 Android 侧向文件夹里写入 `.nomedia`，接着更新 SharedPreferences 中记录的“最近项目”；同时 Desktop 侧由 backend 创建文件夹后，直接生成了一个默认的 `settings.json` 并调用 QML 发送状态刷新。
  *后果：双端项目初始化结构不同，最近项目无法通过工作区同步，行为分叉。*
-  **正确做法**：
  平台层只发送 `ProjectCapability.createProject(workspacePath, "新项目")`。
  Core 收到后，执行以下统一操作：
  1. 创建项目文件夹。
  2. 生成默认的 `.nomedia` 或内部元数据文件。
  3. 更新工作区级 `workspace.json` 的项目列表。
  4. 返回 `success=true` 并包含 `changedEntities = ["ProjectList"]`。
  5. 双端收到 Result Envelope 后，解析出实体列表变动，仅在 UI 上重绘项目列表，两端文件结构完美一致。

### 实例 2：设置变更 (Settings)
- ❌ **错误做法**：
  在 Android 的 `SettingsActivity` 点击保存时，直接使用 Android 的 `SharedPreferences` 保存排版大小，然后再调用 JNI 同步到 Core；而 Desktop 则直接使用 Qt 的 `QSettings` 写文件。
  *后果：同步机制完全无法触及这些偏好设置，用户换机或多端切换时设置直接丢失。*
-  **正确做法**：
  平台层在 Settings UI 界面进行操作时只在本地维护一个内存 draft。当点击保存时，调用 `SettingsCapability.saveLocalSettings(workspacePath, newSettings)`。
  Core 将数据写入到工作区指定的 `settings.json` 中，并在写入成功时广播 `SettingsSaved` 事件。平台端监听该事件，读取最新的有效设置并更新编辑器字体大小。

### 实例 3：同步冲突 (Sync Conflict)
- ❌ **错误做法**：
  在执行同步操作时，发生了 Git 冲突。Desktop backend 抓到 `libgit2` 抛出的 `Merge conflict on chapter_1.txt` 异常字符串，直接在 UI 弹窗打印原始堆栈；Android 抓到后由于解析不出字符串，认为同步超时，提示用户“网络连接失败”。
  *后果：同步状态在双端完全不可理解，Android 侧逻辑吞错且欺骗用户，极易导致二次覆盖写。*
-  **正确做法**：
  Core `SyncCapability.sync()` 底层捕获 `libgit2` 冲突，在 Core 内部计算出冲突路径和差异，返回 Result Envelope：
  `{ "success": false, "errorCode": "SYNC_MERGE_CONFLICT", "status": "conflict", "data": { "conflictFiles": ["chapter_1.txt"] } }`。
  双端均根据 `SYNC_MERGE_CONFLICT` 错误码弹出一致的冲突合并引导对话框，让用户选择解决方案。

### 实例 4：思维导图操作 (MindMap)
- ❌ **错误做法**：
  Android 侧拖动并修改了思维导图节点名字，直接将修改后的 JSON 用文件操作写回 `mind_map.json`，然后重新读取 JSON 并强刷 Canvas。
  *后果：Desktop 端正开着同样的文件但全然不知，没有文件完整性校验，可能随时覆盖或者覆盖掉 Core 的自动布局。*
-  **正确做法**：
  Android 侧修改节点名称，发送 `MindMapCapability.updateNode(workspacePath, projectId, graphId, nodeId, "新节点名称")` 命令。
  Core 在内部图数据结构中修改节点并校验，写回统一的 workspace 并触发 `MindMapGraphUpdated` 事件，双端在收到该事件后高效率更新快照并重新进行裁剪区重绘。

---

## 五、 禁止平台分叉规则 (Anti-Bifurcation Policy)

为保证项目长远的可维护性，严禁出现任何形式的技术方案或业务方案分叉。以下规则是刚性约束，必须由 CI 静态检查、CR 评审和架构巡检共同卡点：

1. **能力对等原则**：禁止任何 Android 侧独占的业务功能在 Desktop 侧无对应 Core API 支持。同样地，禁止 Desktop backend 自己实现一套和 Android 不同的底层业务规则。
2. **禁止平台级业务状态修复**：禁止在 QML 代码或 Android Activity/ViewModel 内部通过硬编码去“修补”、“绕过”或“拼装”业务状态机。
3. **禁止 JNI 编写业务算法**：JNI 和 C++ backend 绑定层严禁包含数据排序、搜索过滤、字符排版计算等算法。此类逻辑必须移入 Core。
4. **禁止多义错误表达**：禁止双端使用不同的自定义字符串去表达同一个状态。必须使用统一的 `errorCode` 和 `SharedStatus`。
5. **禁止 UI 决策业务结果**：禁止通过判断 UI 控件状态、页面是否加载完毕来在本地判断某项业务是成功还是失败。业务最终成功/失败必须有 Core 返回值背书。
6. **禁止长期数据本地分叉**：禁止为了赶进度将任何需要多端同步的长期业务数据只存在某一个平台本地（例如 Android SharedPreferences 或 Desktop `.config`），必须无条件沉淀到 `writer_core` 的工作区定义文件中。
7. **禁止为 demo 绕道**：禁止在赶进度时，通过平台端拼凑假 JSON 或绕过 Core 的 Command 锁机制去直接修改 workspace 底层文件。
8. **同步冲突标准化**：禁止把同步冲突（Git Merge Conflict）仅仅当成 raw 报错字符串打印或吞掉。必须统一映射为 `SharedStatus.conflict`，由 Core 返回受影响实体，平台提供图形化选择逻辑。

---

## 六、 双端一致性检查 Checklist

任何包含跨平台业务变更的 PR，必须强制通过以下 Checklist 的自检与 CR：

* [ ] **Core Capability 优先**：该功能是否已经率先在 Rust Core (`core/writer_core`) 中定义并实现了 request/response？
* [ ] **错误码对齐**：所有可能的异常分支是否都映射到了标准 `errorCode`，并且双端对其处理逻辑一致？
* [ ] **Core 测试覆盖**：该功能的 Core Command / Capability 是否在 Rust 层拥有 100% 跑通的单元测试或集成测试？
* [ ] **Android 桥薄化**：Android 绑定层（`bindings/android`）是否只有少于 50 行的无状态物理桥接代码，且不包含任何业务变量？
* [ ] **Desktop 适配器对齐**：Desktop backend 是否调用了同一个 Core API，返回的结构语义是否与 Android 一致？
* [ ] **状态机归一**：两端使用的 UI 状态机（如同步中、冲突、就绪）是否全量采用 `SharedStatus` 的状态定义？
* [ ] **文件写入安全**：该功能是否完全杜绝了平台端直接向 workspace 写入文件的行为？
* [ ] **同步白名单同步更新**：如果有新增的持久化文件，是否已经将其路径添加到 `core/writer_core` 的同步忽略/包含白名单中？
* [ ] **技术路线同步更新**：是否已根据需要修改了对应平台的 `TECHNICAL_ROUTE.md` 文档？
* [ ] **双端交叉验证**：该修改是否已经在 Android 模拟器（或真机）和 Desktop 桌面端同时进行过同等功能的正反向验收？

---

## 七、 迁移路线 (Migration Roadmap)

针对目前项目已出现的“Android 和 Desktop 分离割裂”的严重架构风险，必须在后续阶段逐步按以下迁移路线将项目拉回正轨：

### 阶段 1：盘点能力 (Capability Auditing)
- **输入**：对现有 `bindings/android`、`apps/android/NativeCoreBridge` 以及 `apps/desktop/src/backend` 中所有的公开方法进行全量盘点。
- **输出**：建立 `Capability Matrix`。对比并找出哪些方法在 Android 有而在 Desktop 没有，或者两者在返回结构和错误码上存在分歧。

### 阶段 2：Core Capability Facade (建立核心能力门面)
- **重构**：在 Rust Core 中，统一对外暴露 `facade` 模块。
- **收敛**：把所有的写操作重构为统一的 Core Command。
- **信封化**：确保所有的 API 调用都使用标准 `ResultEnvelope` 对外通信。

### 3. 阶段 3：绑定层变薄 (Binding Thinning)
- **拆除**：逐步拆除 JNI 和 Desktop backend 中的逻辑校验和临时垫片代码。
- **无状态化**：把绑定层彻底改造成无状态的数据转发层，确保两端行为完全通过 Core 返回的 Payload 决定。

### 4. 阶段 4：双端对齐测试 (Cross-platform Integration Testing)
- **用例复用**：在 CI 中，引入两端测试用例。如同样的 `createProject` 用例在 Kotlin (JNI 方式) 与 C++ (Shared ABI 方式) 下都必须表现出完全等同的断言结果。
- **行为验收**：双端对于 `SyncConflict` 和 `MindMapEdit` 的处理表现出完全一致的交互。

### 5. 阶段 5：支持未来五端 (Five-platform Ready)
- **接入层标准化**：新客户端（如 macOS, Web, iOS）的接入必须只能调用统一导出的 `bindings/shared` 动态链接库。
- **完全共享**：新端上线只需要开发 UI 和适配层，商业和业务层 0 代码改动，完全体现 Rust Core 的跨平台价值。
