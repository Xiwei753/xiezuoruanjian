# 技术路线与架构约束

## 最高优先级规则
- **核心契约约束：** 项目已建立跨平台业务底线契约：[《跨平台能力契约与 Core-first 架构约束》](CROSS_PLATFORM_CAPABILITY_CONTRACT.md)。所有跨平台业务设计与能力对齐，必须以该契约文档为唯一准则。其优先级高于任何单端（Android、Linux、未来新增端）的局部技术路线和实现细节。
- 任何后续提示词、AI 任务、人工改动，如果与本文档或上述契约冲突，以契约和本文档为准。
- 不能在功能实现中私自换技术栈，更不能绕过 Core 在平台端私自建立业务状态机。
- 如需改变路线，必须先单独提交技术路线文档变更，并说明为什么旧路线不再适用。

## 当前仓库事实
- Android：
  - Kotlin + XML/View。
  - AppCompat / Material / ConstraintLayout / Lifecycle。
  - 当前主业务入口是 `AppServiceBridge + UniFFI` 自动生成的 Kotlin 绑定。
  - `NativeCoreBridge` / `writer_core_jni` 仅作为 legacy JSON/JNI fallback，不是新业务入口。
  - 只正式支持 arm64-v8a。
  - 暂不引入 Compose 作为主 UI 技术。
- Linux：
  - 当前是 Qt/QML 路线。
  - 遵循 Qt/KDE 桌面应用路线。
  - 不再随意 Qt5 / Qt6 / Qt Quick / 其他 UI 栈来回切。
- Core：
  - Rust Core 是业务真相来源。
  - 工作区、作品、卷、章节、同步、删除安全等由 Core 兜底。
  - `core/writer_core/src/api/` 是当前跨平台稳定 API 层，`facade::WriterCore` 是内部聚合入口，`app_service.rs` 是 UniFFI adapter。

## 总体原则
- Core 负责业务数据和跨平台逻辑。
- 平台端负责输入、渲染、系统能力接入。
- UI 不保存长期业务真相。
- 不用临时文件或 SharedPreferences 绕过 Core。
- 不在 UI 层假成功。
- 不在 UI 层吞错误。
- 不用过程文档替代架构文档。

## Android 导图技术路线
- 导图不是普通页面，而是大画布图形系统。**导图最终是与正文并列的创作知识图谱，不仅限于章节树结构。**
- 章节结构图只是导图在未自定义时的**一种自动生成视图**。
- 正文和导图通过 anchor 和 link 绑定。
- 数据模型与布局在 Rust Core。**存储也必须由 Rust Core 的 workspace 统一管理**，不允许 Android 私自用 SharedPreferences 等保存长期图数据。
- Android 只拿快照渲染。
- Android 不在每帧访问 Rust Core。
- Android 不在每帧解析 JSON。
- Android 不在每帧重新布局。
- Android 不用 RecyclerView / LinearLayout / 普通 ViewGroup 拼节点。
- Android 不用 WebView 做导图。
- Android 暂不迁移 Compose。
- Compose Canvas 可作为参考，但不是当前主路线。
- 推荐路线：
  - V1：建立 MindMapSnapshot、MindMapRenderer 接口、Android 自定义渲染 View 骨架。
  - V1 可以用硬件加速 Canvas 验证数据链路和交互。
  - 渲染接口必须预留 GLSurfaceView/OpenGL ES 后端。
  - 目标路线是独立渲染 Surface + GPU 批量绘制节点/边/文本纹理。
  - 后续性能不足时升级到 GLSurfaceView/OpenGL ES，不推翻 Core 和 Model。
- 为什么：
  - 普通 View 树无法承受大量节点高频平移缩放。
  - RecyclerView 是列表，不是无限二维画布。
  - WebView 会带来输入延迟、调试复杂、原生能力割裂。
  - Compose 迁移成本高，当前仓库没有 Compose 依赖，不适合为导图单点引入新 UI 栈。
  - OpenGL ES / GLSurfaceView 是 Android 大画布高性能图形路线之一。

## 跨语言暴露层与传输路线 (FFI / UniFFI)
- **当前路线：**
  - Rust Core 是唯一业务真相来源，Android 主业务入口固定为 `AppServiceBridge + UniFFI`。
  - Rust `api/` 模块定义稳定 DTO、错误映射和 `WriterCoreApi` 服务，是 Core API 层收口的事实边界。
  - `core/writer_core/src/api.udl` 只声明 UniFFI 需要暴露的 DTO、错误枚举和 `WriterAppService` 方法；Kotlin `writer_core.kt` 必须由 UniFFI 生成，不得手写业务逻辑。
  - `core/writer_core/src/app_service.rs` 是薄 UniFFI adapter，只保留 Android 兼容的 `WriterAppService` 对象和方法名，并委托 `api::WriterCoreApi`。
  - Android 分层为 `UI/ViewModel -> Repository/Controller -> Workspace/Writing/Settings/Sync/Stats/StarMap/MindMap Bridge -> AppServiceBridge -> UniFFI -> Rust Core`。
  - 当前路线已经从“FFI 主链路收口”进入“Core API 层收口”：新增平台能力应优先落到 Rust `api/`，再由 UniFFI 或其他平台 adapter 暴露。
- **legacy JSON 使用边界：**
  - `NativeCoreBridge` / `writer_core_jni` 仅保留给尚未迁完的 fallback、native 加载状态和少量 legacy 动作路径。
  - 统计、导图、星图等尚返回 JSON 字符串的接口是临时迁移残留，只能封闭在领域 Bridge 内并转换为 `BridgeResult<T>`；不得把裸 JSON 扩散到 UI 作为新契约。
  - 当前收口目标不是把所有 Core 接口一次性 API-ification / typed DTO 化，而是先确保残留 JSON 不越过领域 Bridge 边界。
- **禁止事项：**
  - 禁止把 `NativeCoreBridge + JSON over JNI` 写成当前主路线。
  - 禁止用 JSON 字符串伪装 UniFFI typed API。
  - 禁止把 `api.udl` 当作业务 API 的唯一设计文档或事实来源。
  - 禁止把 `app_service.rs` 继续扩展成 DTO、错误、业务转发混杂的大文件。
  - 禁止手改 UniFFI 生成文件中的业务逻辑。
  - 禁止继续新增手写 `Java_com_xiwei_...` JNI 主业务函数。

## 网络同步路线
- **核心原则：** 拥抱基于 Token 的标准 API，对容易失败的底层代理探测进行“断舍离”。
- **Android 与多端演进：**
  - 由于移动端（Android）和部分受限网络环境（Linux）下，底层 C 语言库（`libgit2`）对系统 VPN 和代理透明转发的支持极差，导致频繁出现 `Certificate (-17)` 和 TCP 阻断问题。
  - **当前确立**：彻底拥抱基于 Token 的 HTTP/REST API（如 GitHub API）。精简乃至删除内核中对底层 22 端口、443 端口的暴力 TCP 探测和 HTTP CONNECT 代理探针。
- **未来扩展能力：**
  - 同步服务保持 `BackendType` 的枚举抽象（Git, GithubApi, WebDav, S3 等）。
  - 网络层只需保证 HTTP Client (如 `reqwest`) 能读取系统级别或用户设定的常规代理配置，网络连接的具体报错直接抛给上层，不在底层过度诊断拦截。

## AI 智能体旁路路线 (AI Agent Bypass)
- **核心原则：** AI 服务不能干扰主线业务数据的原子写入，必须作为“旁路辅助”。
- **行动驱动 (Action-Driven UI)：**
  - AI 不能只返回干瘪的文本回答。
  - AI 模块必须返回结构化的 `AiActionResponse`，包含 `display_text`（给人看的）和 `actions: Vec<AiAction>`（给 UI 画按钮的）。
  - 各端 UI（Android/Linux）收到数据后，在对话框底部渲染原生按钮。用户点击按钮，通过 UniFFI 接口直接调用 Core 的执行函数（如 `navigate_to_settings` 或 `apply_theme`），实现真正的“智能体”体验。

## Android 导图分层职责
- Rust Core：
  - MindMapGraph
  - MindMapNode
  - MindMapEdge
  - MindMapAnchor
  - MindMapLink
  - MindMapLayout
  - MindMapRenderSnapshot
  - 真正作为正文并列图谱的数据由 Core 读写并生成快照。若没有自定义图谱，则退化为从作品/卷/章节自动生成的结构图。
  - 计算 radial tree / horizontal tree 布局。
  - 支持节点与正文片段（MindMapAnchor）的双向绑定，便于后续 AI 扩写及跳转。
- Android Bridge：
  - 主链路通过 `AppServiceBridge + UniFFI` 暴露 typed DTO / typed error。
  - 少量高复杂度统计/图谱快照可暂时返回 JSON 字符串，但必须封闭在领域 Bridge 内。
  - 后续可新增 typed snapshot 或 buffer 传输，但不能把 JSON/JNI fallback 恢复成主路线。
  - 不泄露 token。
  - 不崩溃。
- Android Model：
  - Kotlin data class 映射快照。
  - 管理 Viewport。
  - 管理选中节点。
  - 不保存长期业务数据。
- Android Activity：
  - 只负责页面生命周期、入口、错误显示。
- Android Renderer：
  - 只负责绘制快照。
  - 每帧只更新 viewport matrix。
  - 支持平移、缩放、惯性、点击命中。
  - 缓存 Paint、Path、TextLayout、节点 bounds。
  - 可见区域裁剪。
  - Debug HUD 显示 fps、frame time、visible nodes、total nodes。
- Layout：
  - 只放 toolbar、surface、状态层。
  - 不用复杂嵌套布局堆节点。

## 120fps 性能约束
- 目标设备：骁龙 888 级别。
- 目标体验：小中型导图拖动/缩放尽量贴近 120Hz。
- 每帧预算按 8.33ms 设计。
- 拖动过程中禁止：
  - IO；
  - JSON 解析；
  - Rust Core 调用；
  - 布局计算；
  - 大量 Kotlin 对象分配；
  - 创建 Paint / Path / Rect / Shader；
  - 重新测量所有文本。
- 必须：
  - 支持 requestedFrameRate / 高刷请求；
  - 支持 frame time 统计；
  - 支持可见区域裁剪；
  - 支持文本/路径缓存；
  - 支持布局快照复用。
- 后续 OpenGL ES 路线：
  - 节点和边批量绘制。
  - 文本使用纹理图集或缓存 bitmap。
  - 大量节点不走每节点 View。
  - 后续可接 Android Frame Pacing / Swappy，但不在 V1 强制引入。

## 技术决策记录
- 不用 WebView。
  - 原因：会带来输入延迟、调试复杂、原生能力割裂。
  - 以后如何改变该决策：如果未来需要完全跨平台的 Web 渲染且能接受性能损失，必须先证明原生方案不再适用。
- 不用每节点 Android View。
  - 原因：普通 View 树无法承受大量节点高频平移缩放。
  - 以后如何改变该决策：除非 Android 推出能在普通 View 树上支持无限大画布万级节点高性能的新机制。
- 不把 Compose 作为当前导图主路线。
  - 原因：当前仓库没有 Compose 依赖，迁移成本高，不适合为导图单点引入。
  - 以后如何改变该决策：当整个 Android App 全面迁移 Compose，且 Compose Canvas 性能达标时。
- 不把导图数据写死在 Android。
  - 原因：业务数据和结构应由 Rust Core 统一管理，保证多端一致性。
  - 以后如何改变该决策：如果完全放弃跨平台策略。
- Rust Core 负责图数据和布局。
  - 原因：统一数据结构和布局算法，避免多端重复实现。
  - 以后如何改变该决策：如果布局算法极其依赖特定平台的字体测量且无法在 Core 中实现时，但仍应尽量在 Core 算大局，平台微调。
- Android 渲染层以 Renderer 接口隔离 Canvas / OpenGL 后端。
  - 原因：V1 可先用 Canvas 验证逻辑，后续性能不足时可平滑升级 OpenGL ES。
  - 以后如何改变该决策：不可轻易改变，始终需要接口隔离。
- V1 允许 JSON 快照，但只用于低频加载。
  - 原因：实现成本低，适合早期验证，但大规模数据会卡顿。
  - 以后如何改变该决策：当 JSON 快照成为性能瓶颈（达到前述触发条件）时，升级二进制。
- 如果 JSON 快照成为瓶颈，升级到 DirectByteBuffer / FlatBuffers / Protobuf。
  - 原因：减少内存拷贝和反序列化开销。
  - 以后如何改变该决策：如果发现新一代零拷贝序列化方案更好。
- V1 先做可测骨架，不一次做完整功能。
  - 原因：分步验证，降低风险，避免大重构。
  - 以后如何改变该决策：无。

## 目录级技术路线
除了本全局技术路线外，各个核心目录也定义了各自的实现边界。后续修改代码时，必须遵守对应目录的技术路线：

- **Android 技术路线**：[apps/android/TECHNICAL_ROUTE.md](../apps/android/TECHNICAL_ROUTE.md)
- **Linux 技术路线**：[apps/desktop/TECHNICAL_ROUTE.md](../apps/desktop/TECHNICAL_ROUTE.md)
- **Rust Core 技术路线**：[core/writer_core/TECHNICAL_ROUTE.md](../core/writer_core/TECHNICAL_ROUTE.md)

**冲突处理规则**：
- 如果全局文档和目录文档存在冲突，先以更严格的约束为准。
- 如果需要调整路线，必须先提交文档变更，不能在功能代码里绕开。
