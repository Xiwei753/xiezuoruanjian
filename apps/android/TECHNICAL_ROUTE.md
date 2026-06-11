# Android 技术路线与实现边界

**本目录最高优先级规则**
- 本文档是 apps/android 目录的技术路线约束。
- 后续任何修改本目录的提示词、AI 任务、人工 PR，必须先读取本文档。
- 如果提示词和本文档冲突，以本文档为准。
- 如果确实需要改变路线，必须先提交本文档变更。

## 当前事实
- Android 当前是 Kotlin + XML/View。
- 使用 AppCompat / Material / ConstraintLayout / Lifecycle。
- 主业务入口通过 `AppServiceBridge + UniFFI` 调用 Rust Core typed DTO/error。
- `NativeCoreBridge` / `writer_core_jni` 仅保留 legacy JSON/JNI fallback、native 加载状态和少量旧动作路径。
- 官方支持 ABI 以 arm64-v8a 为准。
- 当前没有 Compose 作为主 UI 技术。

## Android 总原则与全局契约
- **遵守跨平台契约：** 所有的业务接口与状态机表现必须完全符合 [《跨平台能力契约与 Core-first 架构约束》](../../docs/CROSS_PLATFORM_CAPABILITY_CONTRACT.md) 的规定。
- **纯适配器桥梁：** `AppServiceBridge` 是 Android 主业务桥接入口，只做 UniFFI typed DTO/error 与 Kotlin model 的转换；`NativeCoreBridge` 只能作为 legacy fallback。两者都绝对不能包含自身业务规则、规则校验或持久化决策，更不能成为业务真相来源。
- Activity 只负责页面生命周期、入口、权限、错误展示。
- ViewModel / Repository / 领域 Bridge 承担状态映射和底层调用；领域 Bridge 默认依赖 `AppServiceBridge`。
- Stats / (MindMap LEGACY) / StarMap 若仍接收 Core 返回的 JSON 字符串，只能在领域 Bridge 内解析为 typed model + `BridgeResult<T>`；UI/ViewModel 不接收裸 JSON。
- UI 不保存长期业务真相。
- 长期数据必须走 Rust Core / workspace。
- 不用 SharedPreferences 保存长期业务数据。
- 不在 Activity 里堆业务逻辑。
- 不在 UI 层假成功。
- 不在 UI 层吞错误。

## Android 导图路线
- 导图是大画布图形系统，不是普通页面。
- V1 使用自定义 View + Canvas 验证链路。
- 渲染层必须通过 MindMapRenderer / MindMapRenderView 之类接口隔离。**(LEGACY: 以下渲染路线已迁移至 StarMap)**
- 长期预留 SurfaceView / GLSurfaceView / OpenGL ES 后端。
- 不用 WebView。
- 不用 RecyclerView / LinearLayout / 每节点 Android View。
- 不把 Compose 作为当前导图主路线。
- 不在每帧访问 Rust Core。
- 不在每帧解析 JSON。
- 不在每帧重新布局。
- 进入导图时加载 snapshot，交互中只更新 viewport matrix。
- 120fps 目标按 8.33ms 帧预算约束。

## Android 渲染约束
- onDraw/onDrawFrame 不创建大量对象。
- Paint / Path / Rect / Matrix 尽量缓存。
- 文本测量和节点 bounds 尽量缓存。
- 边绘制不得每帧 O(edges * nodes) 查找节点。
- HUD 字符串低频更新。
- 拖动、缩放、fling 使用 postInvalidateOnAnimation。
- 页面不可见时停止无意义刷新。

## Android 编辑器路线
- 当前 `WriterEditText` + overlay 只能作为过渡形态。
- 当前过渡形态不得再通过透明 `ForegroundColorSpan` 或 hidden span 隐藏真实正文文字来做吐字动画。
- `TypingAnimationController` 只允许记录轻量动画事件占位；不得修改正文 `Editable` 的可见性或内容格式。
- 最终路线是 `SujianEditorView : View` 自绘正文、选区、光标和动画。
- 输入法通过 `onCreateInputConnection()` 和自定义 `InputConnection` 接入。
- Kotlin 层不得自行猜跨平台文本 diff；插入、删除、粘贴、撤销、重做和 IME composition 后续必须映射为 Core `EditorTransaction`。
- Renderer 只消费 Core transaction / animation event 和平台文字布局结果。

## Android 路线变更规则
- 若要引入 Compose，必须先改本文档，说明为什么整个 Android UI 需要迁移。
- 若要引入 OpenGL ES，必须保持现有 StarMap Snapshot / Renderer 抽象，不推翻 Core 和 Model。(注：原 MindMapSnapshot 已迁移至 StarMap)
- 若要引入 FlatBuffers / DirectByteBuffer，必须先证明 JSON 快照达到文档里的性能触发条件。
- 若要改同步或存储，必须同时检查 core/writer_core/TECHNICAL_ROUTE.md。
