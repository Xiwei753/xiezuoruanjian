// SujianEditor — 写作区组件宿主。
// 消费 EditorLayoutSnapshot 经 EditorRenderBackend 静态渲染；
// 输入经 EditorSemanticDispatcher 统一排队，IME 连接经 HarmonyImeConnection。
// 用 Stack 组合静态层：selection background → text → composition underline → static caret。
// 不自己维护 cursor timer/animation；光标固定显示，不做闪烁/平滑移动/淡入淡出。
// 动画层以后新增 motion 消费同一份 snapshot，不改 session/input/render 的数据边界。
//
// Issue #629 评论 5 第 5 节：生产接入键盘/鼠标/触控板/手写笔。
// - onKeyEvent → KeyboardInputService.normalize → toSemanticCommand → dispatcher.dispatch
// - onMouse → PointerInputService.normalizeMouseEvent → 点击/drag selection/滚轮 viewport（经 dispatcher）
// - onTouch → 真实 drag selection（TouchDown 计 anchor, TouchMove drag, TouchUp 结束）（经 dispatcher）
// - 笔事件由 StylusInputService 识别后按触控选择/书写区交互处理
// 原始平台事件（KeyEvent/MouseEvent/TouchEvent/StylusEvent）不进 Rust，转成语义命令。
//
// Issue #629 评论 5 第 8 节：真实可滚动 viewport。
// - 用 Scroll(scroller) 包裹内容，正文+选区+composition+caret 在同一滚动坐标系里。
// - onScroll 回调把 viewport 状态回调给 WritingScreen（onViewportChange）。
// - restoreViewport(state) 由 WritingScreen 在接续加载完成、layout 就绪后调用。
//
// Issue #629 评论6 Part A+D：统一串行边界 + 删除重复 onSelectionTap。
// - 物理键盘/鼠标/触摸都经 EditorSemanticDispatcher 派发语义命令，不再直接调 EditorInputAdapter/SelectionController。
// - 删除 onSelectionTap prop（双 owner）：TouchUp 只经 dispatcher/SelectionController 发一次 selection。
// - dispatchKeyCommand 改为派发语义命令（insertText/graphemeBackspace/graphemeLeft 等），
//   修正旧参数调用（onTextInput/onDelete 旧签名已改）。
//
// Issue #629 评论6 Part C：viewport 恢复闭环 + 分开 viewport/content 尺寸。
// - 旧实现：emitViewportChange() 把 viewportHeight = textHeight、contentHeight = textHeight（两者永远相同），
//   maxScrollTop 永远 0；Scroll 内 Stack 固定 .height('100%')，长正文真实内容高度被压成视口高度。
// - 新实现：分开记录外层 Scroll 可视区域尺寸（viewportWidth/viewportHeight，来自 Scroll onAreaChange）
//   和内层正文实际内容尺寸（contentWidth/contentHeight，来自 Text onAreaChange）。
//   emitViewportChange() 用真实 viewport + content 尺寸，maxScrollTop = contentHeight - viewportHeight 可正确 > 0。
//   Scroll 内 Stack 不再固定 .height('100%')，改为按 max(viewportHeight, contentHeight) 动态高度，
//   至少 viewport 高度，随正文增长。
// - 父子恢复通道：viewportController prop 由 WritingScreen 注入；aboutToAppear 注册 scrollTo + getDimensions，
//   aboutToDisappear unregister。删除旧 restoreViewport(state) 方法（恢复改为通过 controller）。

import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { EditorRenderBackend, EditorRenderResult } from '../render/EditorRenderBackend'
import { SelectionRect, CaretRect, CompositionUnderlineRect, LineLayout } from '../render/editor_render_geometry'
import { HarmonyImeConnection } from '../input/HarmonyImeConnection'
import { EditorInputAdapter } from '../input/EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'
import { EditorSemanticDispatcher, SemanticEditCommand } from '../input/EditorSemanticDispatcher'
import { LineNavigationResolver, LineLayoutState, NavigationLine } from '../selection/LineNavigationResolver'
import { CaretAffinity } from '../render/editor_layout_math'
import { VisualCaretPosition } from '../render/editor_layout_math'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { KeyboardInputService, PlatformKeyEvent, SemanticKeyCommand } from '../../../platform/input/KeyboardInputService'
import { PointerInputService, PlatformPointerEvent } from '../../../platform/input/PointerInputService'
import { StylusInputService } from '../../../platform/input/StylusInputService'
import { EditorViewportState } from '../render/EditorViewportState'
import { EditorViewportController, ViewportDimensions } from '../render/EditorViewportController'
import { EditorTextStyleProjector, EditorStyleConfig } from '../render/EditorTextStyleProjector'
import { PlatformApiResolver } from '../../../platform/version/PlatformApiResolver'
import { EditorRuntimeSnapshotStore, EditorRuntimeSnapshot, CaretRectSnapshot, CurrentLineMetricsSnapshot } from '../diagnostics/EditorRuntimeSnapshotStore'
import { inspector } from '@kit.ArkUI'
import hilog from '@ohos.hilog'
// Issue #879：运动层相关类型。
import { EditorPresentationUpdate } from '../motion/EditorPresentationUpdate'
import { EditorVisualFrame } from '../motion/EditorVisualFrame'
import { EditorMotionPolicy, MotionMode } from '../motion/EditorMotionPolicy'
import { EditorMotionCoordinator, DisplayedSnapshotContext, PreparedCandidate } from '../motion/EditorMotionCoordinator'
import { RevisionPositionMap, PositionMapStep } from '../motion/editor_revision_position_map'
import { EditorMotionLayer, ClipRect, MotionTextOverlayBuilder, MotionTextOverlayParams, MotionWindowSource } from '../motion/EditorMotionLayer'
// Issue #879 最新复核评论修复（问题1）：EditorEditResult 用于收集事务链。
import { EditorEditResult } from '../../../corebridge/dto/EditorDtos'
// Issue #879 复核评论修复（问题1/3/4）：前后台排版隔离 + 稳定运动字形源池。
import { EditorPendingLayoutHost, PendingLayoutRequest } from './EditorPendingLayoutHost'
import { EditorMotionSourcePool, MotionSourceRoots } from '../motion/EditorMotionSourcePool'
// Issue #879 本轮复核评论修复（问题6）：用 getUIContext().createAnimator 替代 Animator.create。
// Animator.create 从 API18 起废弃。@kit.ArkUI 导出 AnimatorOptions 和 AnimatorResult。
// 创建 animator 改为 this.getUIContext().createAnimator(options)。
import { AnimatorResult, AnimatorOptions } from '@kit.ArkUI'
// PathShape 用于主 Text 的静态可见区 clipShape 裁切（问题1）。
import { PathShape } from '@ohos.arkui.shape'
// ArkUI KeyEvent / MouseEvent 是 component/common.d.ts 全局声明，不从 @kit.InputKit 导入。
// .onKeyEvent / .onMouse 回调给的是 ArkUI 事件类型，非 InputKit 类型。

const EDITOR_LOG_DOMAIN = 0x0000
const EDITOR_LOG_TAG = 'SujianEditor'
const INPUT_HOST_ID = 'sujian-editor-input-host'
// Issue #768 评论5837824666：正文 Text 的固定 id，用于 ComponentObserver 监听布局完成回调。
const EDITOR_TEXT_ID = 'sujian-editor-text'
// Issue #776 评论5848626733 第8项：正文阅读边距（vp）。
// 文字、selection、composition、caret 都在同一个带 padding 的内容容器里，
// 光标不会和文字错位，正文不贴着标题栏和屏幕边缘。
const EDITOR_HORIZONTAL_PADDING_VP = 20
const EDITOR_TOP_PADDING_VP = 20
const EDITOR_BOTTOM_PADDING_VP = 32

/**
 * Issue #879 问题4：两阶段发布——暂存显示切换数据。
 * 运动窗口 layoutReady 之前不切换主 Text/静态遮罩，暂存切换数据。
 */
interface PendingDisplayData {
  latestUpdate: EditorPresentationUpdate
  lineLayouts: LineLayout[]
  newStyledString: StyledString | null
}

/**
 * Issue #879 复核评论修复（问题1）：候选 token——标识一次候选的完整身份。
 *
 * pendingCandidate、pendingDisplayData、durationMs 归属于同一个 candidateToken。
 * 每次收到同会话的新 presentationUpdate，立刻使旧候选失效并释放其独占 staging 窗口，
 * 但保留从 displayed revision 开始的全部 pendingPresentationUpdates 事务链。
 *
 * Issue #879 复核评论6075662695问题2：**等待状态挂在候选自己身上**——
 * 启动动画的时长放在 token 里，不再用一个会被多个窗口回调抢占的全局
 * pendingAnimationDurationMs。窗口 layout 回调只能推进「属于当前候选帧」的窗口，
 * 旧 displayed / 旧候选窗口的回调既不能提交版本，也不能吞掉新候选的启动时长。
 */
interface CandidateToken {
  /** Core 编辑会话 ID */
  sessionId: number
  /** 候选对应的正文 revision */
  revision: number
  /** 排版请求 token（pendingLayoutToken） */
  layoutToken: number
  /** 候选 generation（每次创建新候选递增） */
  generation: number
  /** 候选自己的动画时长（窗口 ready 后用它启动 animator；0 = 无动画） */
  durationMs: number
}

@Component
export struct SujianEditor {
  // Issue #879：接收 EditorPresentationUpdate（含 layoutSnapshot + editResult + sessionId），
  // 不再只接收 EditorLayoutSnapshot。
  @Prop @Watch('onPresentationUpdateChanged') presentationUpdate: EditorPresentationUpdate | null = null
  // Issue #879：运动策略，由父组件（WritingScreen）从 LocalSettings 构造后传入。
  motionPolicy: EditorMotionPolicy = new EditorMotionPolicy(
    100, 80, MotionMode.Independent, false, false, false, false
  )
  // 从 presentationUpdate.layoutSnapshot 同步出来的当前布局快照，
  // 供所有内部方法消费（替代旧的 this.currentSnapshot）。
  // Issue #879 最新复核评论修复（问题2）：currentSnapshot 只在 executeDisplaySwitch 中
  // 与 displayedSnapshot 一起更新——不再在 onPendingLayoutReady 中提前更新。
  // 后台候选等待期间，hitTest/cursor 使用 displayedSnapshot（对应主 Text 的 LayoutManager）。
  @State currentSnapshot: EditorLayoutSnapshot | null = null
  // Issue #879 最新复核评论修复（问题2）：Core 最新编辑事实——
  // onPendingLayoutReady 中保存 latestUpdate.layoutSnapshot 到此字段，
  // 供向 Core 发送命令时做版本映射（displayedSnapshot → latestCoreSnapshot）。
  // currentSnapshot 不再在 onPendingLayoutReady 中提前更新，改用此字段。
  private latestCoreSnapshot: EditorLayoutSnapshot | null = null
  // Issue #879：运动协调器——接收编辑更新，生成运动规划，输出视觉帧。
  private motionCoordinator: EditorMotionCoordinator = new EditorMotionCoordinator()
  // Issue #879：当前视觉帧——描述某一时刻的完整视觉状态（光标位置、静态/运动层字形范围）。
  // Issue #879 复核评论修复（问题1）：拆分为 displayedVisualFrame 和 candidateVisualFrame。
  // displayedVisualFrame 唯一负责当前可见的运动窗口、光标、静态裁切。
  // candidateVisualFrame 只用于后台准备新 Text 源——候选窗口在同一 ForEach 中以 opacity 0 渲染，
  // 候选源全 ready 后原子提交（displayedVisualFrame = candidateVisualFrame）。
  @State private displayedVisualFrame: EditorVisualFrame | null = null
  @State private candidateVisualFrame: EditorVisualFrame | null = null
  // Issue #879 最新复核评论问题4：候选句柄——prepareCandidate 返回的不可变候选。
  // executeDisplaySwitch 用它一次性提交（而不是从 Coordinator 读“当前计划”）。
  private pendingCandidate: PreparedCandidate | null = null
  // Issue #879 第三轮复核修复：逐帧动画驱动——用 AnimatorResult.onFrame 替换 animateTo + @Watch。
  // animateTo 改的是 UI 状态目标值，@Watch 只看状态赋值，不会把渲染层每帧插值值回写。
  // AnimatorResult.onFrame(progress) 是 ArkUI 渲染层逐帧回调，每帧同步 visualFrame。
  private motionAnimator: AnimatorResult | null = null
  private motionGeneration: number = 0
  @State private isMotionAnimating: boolean = false
  // Issue #879 复核评论修复（问题4）：staticClipCommands 默认值改为全尺寸矩形 path（不裁切），
  // 而非空字符串——空字符串在内容尺寸为零的挂载阶段会构建宽高为 0 的矩形。
  // 静态模式（非动画）不做裁切，只有确切有 motion owner 时才在真实宽高及行布局就绪后计算遮罩。
  @State private staticClipCommands: string = 'M0 0 L99999 0 L99999 99999 L0 99999 Z'
  // Issue #879 复核评论修复：延迟到 layout observer 后才做运动规划。
  // onPresentationUpdateChanged 暂存 pendingPresentationUpdate，等 layout 回调后才调 onPendingLayoutReady。
  // Issue #879 最新复核评论修复（问题1）：改为数组保留连续编辑事务链——
  // 连续快速输入时，多笔 editResult 按顺序保存，onPendingLayoutReady 中合并消费，
  // 不再只保留最新一个导致中间事务被丢弃。
  private pendingPresentationUpdates: EditorPresentationUpdate[] = []
  // Issue #879 复核评论修复：保存旧 snapshot 供 acceptUpdate 使用。
  private previousSnapshot: EditorLayoutSnapshot | null = null
  // Issue #879 第三轮复核修复：保存旧布局行几何——在 refreshRender 之前缓存，
  // 因为 refreshRender 会把 renderResult 更新为新布局（lines=[]），
  // layout observer 回调中再读 renderResult?.lines 已经是空的了。
  private previousLayout: LineLayout[] = []
  // Issue #879 复核评论修复：运动层 old/new 字形源（StyledString）。
  // oldStyledString = 编辑前的正文 StyledString（吞字时显示旧字）
  // newStyledString = 编辑后的正文 StyledString（吐字时显示新字）
  private oldStyledString: StyledString | null = null
  private newStyledString: StyledString | null = null
  // Issue #879 复核评论修复（问题1/3/4）：前后台排版隔离。
  // displayedPresentation 是当前正在显示的，pendingPresentation 是等待后台排版的。
  private displayedPresentation: EditorPresentationUpdate | null = null
  // Issue #879 复核评论修复（问题3）：displayed/pending 几何隔离。
  // displayedSnapshot = 当前正在显示的快照（主 Text observer 只用这个生成渲染几何）
  // displayedLineLayouts = 当前正在显示的行布局（previousLayout 从这个取，不被 pending 覆盖）
  // displayedStyledString = 当前正在显示的 StyledString（applyStyledText 用这个写入主 Text）
  private displayedSnapshot: EditorLayoutSnapshot | null = null
  private displayedLineLayouts: LineLayout[] = []
  private displayedStyledString: StyledString | null = null
  // Issue #879 复核评论修复（问题1）：排版请求——通过 @State + @Prop 传给 EditorPendingLayoutHost，
  // @Watch 自动触发排版。token 用于校验回调对应哪次排版请求。
  @State private pendingLayoutRequest: PendingLayoutRequest | null = null
  private pendingLayoutToken: number = 0
  // Issue #879 最新复核评论修复（问题5）：删除 editorSessionId（Date.now()）——
  // 改用 presentationUpdate.sessionId 作为排版请求的 sessionId，
  // pendingSnap.revision 作为排版请求的 revision。
  // Issue #879 复核评论修复（问题1）：token → StyledString 映射——父组件维护原始引用，
  // EditorPendingLayoutHost 通过 resolveLayoutRequest(token) 回调获取，不走 @Prop 深拷贝。
  private styledStringByToken: Map<number, StyledString> = new Map()
  // Issue #879 复核评论修复（问题3）：已消费的 token 集合——防止同一 token 重复消费。
  private consumedTokens: Set<number> = new Set()
  // Issue #879 复核评论修复（问题6）：后台排版完成的行布局——作为 Coordinator 和
  // computeStaticMaskRects 的唯一相同输入，不用 renderResult.lines（可能为空）。
  private committedLineLayouts: LineLayout[] = []
  // Issue #879 复核评论修复（问题3）：运动字形源池——替代旧的 oldTextControllers/newTextControllers。
  // 源池用 sourceRevision + stableRunId 保存 StyledString/TextController，
  // ForEach key 基于稳定 run 身份（runId），而非一次性自增事务编号。
  private motionSourcePool: EditorMotionSourcePool = new EditorMotionSourcePool()
  // Issue #879 最新复核评论修复（问题3）：窗口级 ComponentObserver 管理——
  // 每个 renderNodeKey 对应一个独立的 ComponentObserver，监听运动窗口 Text 的 layout 事件。
  // @Builder 函数中不能直接创建 ComponentObserver——通过 onWindowLayoutReady 回调
  // 让 SujianEditor 创建 observer，layout 回调中调用 markWindowLayoutReady + markWindowMounted。
  // Text.onDisAppear 中通过 onWindowDisappear 回调通知 SujianEditor 清理 observer 和窗口状态。
  private windowLayoutObservers: Map<string, inspector.ComponentObserver> = new Map()
  private windowLayoutCallbacks: Map<string, () => void> = new Map()
  // Issue #879 本轮复核评论修复（问题4）：两阶段发布——pending 显示切换数据。
  // 运动窗口 layoutReady 之前不切换主 Text/静态遮罩，暂存切换数据，
  // 等窗口 ready 后在 executeDisplaySwitch 中统一执行。
  private pendingDisplayData: PendingDisplayData | null = null
  // Issue #879 复核评论修复（问题1）：候选 token——标识当前候选的完整身份。
  // pendingCandidate、pendingDisplayData、候选动画时长归属于同一个 candidateToken。
  // 每次收到同会话的新 presentationUpdate，立刻使旧候选失效，但保留 pendingPresentationUpdates 事务链。
  // 提交走 tryCommitCandidate：token 只用于身份核对，窗口回调按「是否属于当前候选帧」判定归属。
  private candidateToken: CandidateToken | null = null
  // Issue #879 复核评论修复（问题1）：候选 generation——每次创建新候选递增。
  // 用于 CandidateToken.generation 字段，确保每次候选有唯一身份。
  private candidateGeneration: number = 0
  // Issue #879 最新复核评论修复（问题5）：保存从 onPendingLayoutReady 回调获取的 firstCaretRect——
  // 空正文时由 EditorPendingLayoutHost 从 LayoutManager 计算的光标位置，
  // 传递给 Coordinator 的 acceptUpdateWithChain，空正文时优先使用而非估算。
  private pendingFirstCaretRect: CaretRect | null = null
  // Issue #879 本轮复核评论修复（问题4）：pending RenderPlan 身份字段。
  // layout observer 回调中校验 pending 的 revision/generation/composition/style/width 匹配后才消费。
  // 需保留 per-revision 的旧布局，不要在两次更新尚未消费之间用 previousLayout 一个变量覆盖旧版来源。
  private pendingRevision: number = 0
  private pendingGeneration: number = 0
  private pendingCompositionGeneration: number = -1
  private pendingFontSize: number = 0
  private pendingLineSpacing: number = 0
  private pendingContentWidth: number = 0
  // IME 连接、输入/选区控制器、统一语义调度器由父组件注入；
  // SujianEditor 在获得/失去焦点时 attach/detach IME。
  // Issue #629 评论6 Part A：所有输入经 dispatcher 统一排队，不再直接调 inputAdapter/selectionController。
  harmonyImeConnection: HarmonyImeConnection | null = null
  inputAdapter: EditorInputAdapter | null = null
  selectionController: SelectionController | null = null
  dispatcher: EditorSemanticDispatcher | null = null
  // viewport 状态变化回调（供 WritingScreen 持续写入接续状态）。
  onViewportChange: ((state: EditorViewportState) => void) | null = null
  // Issue #629 评论6 Part C：父子 viewport 恢复通道。WritingScreen 持有 controller 实例并注入；
  // SujianEditor 在 aboutToAppear 注册 scrollTo + getDimensions，aboutToDisappear unregister。
  viewportController: EditorViewportController | null = null
  // 静态渲染颜色：由系统主题资源驱动，light/dark 模式自动切换。
  // Issue #767：旧值硬编码（#33007AFF/#FF007AFF/#FF000000），深色模式下光标不可见、选区/composition 固定蓝色。
  // 改为 ResourceColor 引用系统颜色资源，随系统 light/dark 自动适配。
  selectionBackgroundColor: ResourceColor = $r('sys.color.ohos_id_color_text_highlight_bg')
  compositionUnderlineColor: ResourceColor = $r('sys.color.ohos_id_color_emphasize')
  caretColor: ResourceColor = $r('sys.color.ohos_id_color_text_primary')
  // Issue #879 本轮复核评论修复（问题1）：删除 editorBackgroundColor——不再用背景色 Rect 遮罩。
  // 主 Text 仍完整排版，但其绘制输出按 staticVisibleRanges 用 clipShape/maskShape 裁切——
  // 只显示未被运动接管的范围。
  @State renderResult: EditorRenderResult | null = null
  // 静态渲染几何（从 renderResult 同步出来，供 build() 直接绑定）。
  @State selectionRects: SelectionRect[] = []
  @State caretRect: CaretRect | null = null
  @State compositionUnderlineRects: CompositionUnderlineRect[] = []
  // Issue #629 评论6 Part C：分开 viewport 与 content 尺寸。
  // viewportWidth/viewportHeight：外层 Scroll 可视区域尺寸（px），来自 Scroll onAreaChange。
  // contentWidth/contentHeight：内层正文实际渲染尺寸（px），来自 Text onAreaChange。
  // 旧实现 containerWidth/textHeight 混用，emitViewportChange 把 viewportHeight = textHeight、
  // contentHeight = textHeight（两者永远相同），maxScrollTop 永远 0。新实现分开记录。
  @State viewportWidth: number = 0
  @State viewportHeight: number = 0
  @State contentWidth: number = 0
  @State contentHeight: number = 0
  // viewport 滚动控制器（ArkUI Scroller）。controller.restore 经此 scrollTo。
  // Issue #778 任务8：scroller 由父组件（WritingScreen）注入，HDS 壳层绑定同一 scroller 做沉浸联动。
  scroller: Scroller = new Scroller()
  // Issue #768 任务9：TextController 让 ArkUI Text 的 LayoutManager 成为唯一布局真源。
  // 正文 Text 通过 controller 接收 StyledString（含字号/行高/首行缩进样式），
  // LayoutManager 提供真实行布局/命中测试，消除自算 measureText 与系统布局的错位。
  private textController: TextController = new TextController()
  // 输入服务单例。
  private keyboardService: KeyboardInputService = KeyboardInputService.getInstance()
  private pointerService: PointerInputService = PointerInputService.getInstance()
  private stylusService: StylusInputService = StylusInputService.getInstance()
  // Issue #629 评论11 第2项：行级导航解析器。
  // SujianEditor 持有 renderResult，每次渲染结果更新后同步给 resolver。
  // Dispatcher 行导航命令出队时通过 resolver 读取最新行布局计算目标位置。
  private lineResolver: LineNavigationResolver = new LineNavigationResolver()
  // Issue #629 R7：不再缓存 visual caret affinity。
  // SelectionController.getVisualCaret(cursorUtf16) 是唯一 owner，
  // refreshRenderLayout() 每次按当前 snapshot cursor 从 owner 取，
  // 不再有第二份状态。
  // Issue #629 评论5324447292 第1项：drag anchor 持有完整 VisualCaretPosition（含 affinity），
  // 不丢弃命中测试的 soft-wrap 亲和性。
  // Issue #879 最新复核评论问题2：dragAnchor 保存「可见修订」坐标，只用于判断是否单击
  // （与命中测试结果同坐标系）；真正发给 dispatcher 的 anchor 是 dragAnchorCore
  // （已映射到 Core 最新修订），避免拖选期间跨修订错位。
  private dragAnchor: VisualCaretPosition | null = null
  private dragAnchorCore: VisualCaretPosition | null = null
  private isDragging: boolean = false
  // Issue #768：编辑器焦点状态，供诊断快照读取。
  private isFocused: boolean = false
  // Issue #768：布局来源标记，供诊断快照读取。
  // 'system' = 用系统 LayoutManager，'fallback' = 自计算，'none' = 无布局。
  private lastLayoutSource: string = 'none'
  // Issue #768 评论5835171266 第5项：真实记录布局错误和 fallback 次数。
  private lastLayoutError: string | null = null
  private fallbackCount: number = 0
  // Issue #768 评论5837229456：Text 组件挂载状态标记。
  // API15 之前，在 aboutToAppear 里通过 TextController.setStyledString() 设置内容无法显示，
  // 因为组件还未创建。用 textMounted 守卫确保只在 Text.onAppear 之后才下发 StyledString。
  private textMounted: boolean = false
  // Issue #768 评论5837824666：系统布局完成回调观察者。
  // setStyledString 后不能同步读 LayoutManager（可能拿到上一版布局）。
  // 用 ComponentObserver 'layout' 回调确保只在系统布局完成后才读取 LayoutManager。
  private textLayoutObserver: inspector.ComponentObserver | null = null
  private textLayoutCallback: (() => void) | null = null
  // Issue #768 评论5837824666：布局身份字段。
  // 记录当前已发布的布局对应的 snapshot 身份，供诊断快照判断是否出现"新 revision + 旧 layout"。
  private layoutRevision: number = 0
  private layoutGeneration: number = 0
  private layoutCompositionGeneration: number = 0
  private visualCaretAffinity: string = 'downstream'
  // Issue #768 评论5837824666：缓存上一次布局完成后的行度量和行数，
  // 供 pushDiagnosticsSnapshot 在布局未完成时使用（避免读到旧 LayoutManager）。
  private lastSystemLayoutLineCount: number = 0
  private lastCurrentLineMetrics: CurrentLineMetricsSnapshot | null = null

  aboutToAppear(): void {
    hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'aboutToAppear')
    // Issue #879：从 presentationUpdate 同步 currentSnapshot 并初始化视觉帧。
    if (this.presentationUpdate !== null) {
      this.currentSnapshot = this.presentationUpdate.layoutSnapshot
      this.displayedPresentation = this.presentationUpdate
      // Issue #879 本轮复核评论修复（问题2）：在 aboutToAppear 中也构造 newStyledString，
      // 确保 Text.onAppear → applyStyledText 时 newStyledString 已就绪。
      // onPresentationUpdateChanged 中也会构造，但 @Watch 回调可能在 Text.onAppear 之后才触发。
      const config: EditorStyleConfig = {
        fontSize: this.currentSnapshot.fontSize,
        lineSpacingMultiplier: this.currentSnapshot.lineSpacingMultiplier,
        autoIndentEnabled: this.currentSnapshot.autoIndentEnabled,
        autoIndentWidth: this.currentSnapshot.autoIndentWidth,
      }
      this.newStyledString = EditorTextStyleProjector.projectToStyledString(
        this.currentSnapshot.text, config
      )
      // Issue #879 复核评论修复（问题3）：初始化 displayed 版本——
      // displayedSnapshot/displayedStyledString 与 currentSnapshot/newStyledString 同步。
      this.displayedSnapshot = this.currentSnapshot
      this.displayedStyledString = this.newStyledString
      // Issue #879 最新复核评论修复（问题2）：初始化 latestCoreSnapshot。
      this.latestCoreSnapshot = this.currentSnapshot
    }
    this.refreshRender()
    // Issue #879 复核评论修复（问题4）：初始化静态裁切 path 为全尺寸矩形（不裁切）。
    this.updateStaticClipCommands()
    // Issue #879 复核评论修复：不立即调 onPendingLayoutReady——Text layout 尚未完成。
    // 暂存到 pendingPresentationUpdates，等 Text.onAppear → attachTextLayoutObserver → layout 回调后才消费。
    if (this.presentationUpdate !== null) {
      this.pendingPresentationUpdates.push(this.presentationUpdate)
    }
    // Issue #629 评论11 第2项：注册行级导航解析器到 Dispatcher。
    // SujianEditor 持有 renderResult；每次渲染更新后同步给 resolver。
    // Dispatcher 行导航命令出队时通过 resolver 读最新行布局计算目标。
    if (this.dispatcher) {
      this.dispatcher.registerLineResolver(this.lineResolver)
    }
    // Issue #629 R7：订阅 SelectionController visual caret 变化，只触发重新布局。
    // 不再缓存 affinity——refreshRenderLayout() 每次从 owner 按当前 cursor 取。
    if (this.selectionController) {
      this.selectionController.registerVisualCaretListener((_pos: VisualCaretPosition | null) => {
        this.refreshRenderLayout()
        // Issue #768：光标/选区变化时推送诊断快照。
        this.pushDiagnosticsSnapshot()
      })
    }
    // Issue #629 评论6 Part C + 评论7 第4项：注册父子 viewport 恢复通道。
    // scrollTo 由 scroller.scrollTo 实现；getDimensions 返回当前真实 viewport/content 尺寸。
    // register 不立即消费 pending（尺寸可能为 0）；onAreaChange 拿到真实尺寸后
    // 调 notifyDimensionsChanged 才消费 pending（评论7 第4项）。
    if (this.viewportController) {
      this.viewportController.register(
        (xOffset: number, yOffset: number): void => {
          this.scroller.scrollTo({ xOffset: xOffset, yOffset: yOffset })
        },
        (): ViewportDimensions => {
          return {
            viewportWidth: this.viewportWidth,
            viewportHeight: this.viewportHeight,
            contentWidth: this.contentWidth,
            // Issue #776 评论5849108212 问题5：返回 Scroll 内容总高度（含 padding）。
            contentHeight: this.contentHeight + EDITOR_TOP_PADDING_VP + EDITOR_BOTTOM_PADDING_VP,
          }
        }
      )
    }
    // Issue #768：会话创建时推送诊断快照。
    this.pushDiagnosticsSnapshot()
  }

  aboutToDisappear(): void {
    hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'aboutToDisappear')
    // Issue #879 本轮复核评论修复（问题6）：用 stopActiveAnimation 统一停止动画。
    // 先增 generation 使旧 animator 的 onFrame/onFinish 回调失效，再 cancel。
    this.stopActiveAnimation()
    // Issue #879 复核评论修复（问题1）：清空候选帧。
    this.candidateVisualFrame = null
    // Issue #879 最新复核评论问题4：候选句柄一并清空——不再有可提交的候选。
    this.pendingCandidate = null
    // Issue #879 复核评论修复（问题1）：清空候选 token。
    this.candidateToken = null
    // Issue #879 复核评论修复（问题3）：清空运动源池。
    this.motionSourcePool.clear()
    // Issue #879 复核评论6077187962 问题4：清空窗口实例 ID 映射。
    this.motionCoordinator.clearWindowInstances()
    // Issue #879 复核评论问题5：清空源排版记录缓存。
    this.motionCoordinator.clearSourceLayoutRecords()
    // Issue #879 最新复核评论修复（问题3）：清理窗口级 ComponentObserver。
    this.cleanupAllWindowsObservers()
    this.pendingFirstCaretRect = null
    // Issue #879 复核评论修复（问题1）：清空 token→StyledString 映射。
    this.styledStringByToken.clear()
    // Issue #879 复核评论修复（问题3）：清空已消费的 token 集合。
    this.consumedTokens.clear()
    // Issue #768 评论5837824666：兜底注销布局观察者（onDisAppear 可能未触发）。
    this.detachTextLayoutObserver()
    // Issue #629 评论11 第2项 + 评论15 第3项：取消行导航等待 + 注销行级导航解析器。
    this.lineResolver.cancelWait()
    if (this.dispatcher) {
      this.dispatcher.unregisterLineResolver()
    }
    // Issue #629 评论18：注销 visual caret 监听器。
    if (this.selectionController) {
      this.selectionController.unregisterVisualCaretListener()
    }
    // Issue #629 评论6 Part C：注销父子 viewport 恢复通道。
    if (this.viewportController) {
      this.viewportController.unregister()
    }
    // Issue #768 评论5835171266 第5项：不清空快照，标记编辑器已卸载。
    // 保留最后快照供离页后导出诊断；下次打开编辑器时 updateSnapshot 会覆盖。
    EditorRuntimeSnapshotStore.getInstance().markEditorUnmounted()
  }

  // Issue #768 评论5837824666：用 ArkUI ComponentObserver 的 layout 回调
  // 作为系统几何刷新入口，确保 setStyledString 后等待布局完成才读取 LayoutManager。
  // 华为官方文档明确：文本内容变更后，需等待布局完成才可获取到最新的布局信息。
  // ComponentObserver 从 API10 起可用，项目 compatibleSdk=12 可以直接用。
  private attachTextLayoutObserver(): void {
    if (this.textLayoutObserver !== null) {
      return
    }
    const observer = this.getUIContext()
      .getUIInspector()
      .createComponentObserver(EDITOR_TEXT_ID)
    const callback = (): void => {
      // Issue #879 复核评论修复（问题3）：主 Text observer 只用 displayed 版本——
      // 检查 displayedSnapshot 而非 currentSnapshot（后者可能已被 pending 更新覆盖）。
      if (!this.textMounted || this.displayedSnapshot === null) {
        return
      }
      // 到这里 Text 已完成本轮 layout，才读取 LayoutManager。
      this.refreshRenderLayout()
      // Issue #879 复核评论修复（问题2）：主 Text 的 observer 只更新当前已发布版本的几何、
      // 光标和滚动，不再消费任何 pending 编辑。
      // 删除旧的 pendingPresentationUpdate 消费通路——只让 EditorPendingLayoutHost.onLayoutReady
      // 提交编辑版的 RenderPlan，避免两个入口争抢同一个 pending 更新。
    }
    observer.on('layout', callback)
    this.textLayoutObserver = observer
    this.textLayoutCallback = callback
  }

  private detachTextLayoutObserver(): void {
    if (this.textLayoutObserver !== null && this.textLayoutCallback !== null) {
      this.textLayoutObserver.off('layout', this.textLayoutCallback)
    }
    this.textLayoutObserver = null
    this.textLayoutCallback = null
  }

  // Issue #768：把当前编辑器运行时状态推送到诊断快照存储。
  // 从 displayedSnapshot、几何状态、IME 状态、滚动状态等读取数据构造完整快照。
  // 不写正文内容，只写长度、offset、几何和状态。在各生命周期点调用。
  // Issue #879 最新复核评论修复（问题2）：使用 displayedSnapshot 而非 currentSnapshot——
  // 诊断快照应反映当前显示的内容。
  private pushDiagnosticsSnapshot(): void {
    const store = EditorRuntimeSnapshotStore.getInstance()
    const snap = this.displayedSnapshot ?? this.currentSnapshot

    // 滚动偏移
    let scrollX: number = 0
    let scrollY: number = 0
    try {
      const offset = this.scroller.currentOffset()
      scrollX = offset.xOffset
      scrollY = offset.yOffset
    } catch (_e) {
      // scroller 未就绪时保持 0
    }
    const imeAttached: boolean = this.harmonyImeConnection !== null && this.harmonyImeConnection.isAttached()
    const apiLevel: number = PlatformApiResolver.getInstance().getApiLevel()

    // 无活跃会话时推送 hasActiveSession=false 的快照，仍记录视口/IME/焦点/滚动状态。
    if (snap === null) {
      const emptySnapshot: EditorRuntimeSnapshot = {
        hasActiveSession: false,
        revision: 0,
        generation: 0,
        textUtf8Bytes: 0,
        textUtf16Length: 0,
        cursorByteOffset: 0,
        cursorUtf16Offset: 0,
        selectionAnchorByte: 0,
        selectionHeadByte: 0,
        hasComposition: false,
        compositionStartByte: -1,
        compositionEndByte: -1,
        fontSizeVp: 0,
        lineSpacingMultiplier: 0,
        effectiveLineHeightVp: 0,
        firstLineIndentEnabled: false,
        firstLineIndentChars: 0,
        effectiveFirstLineIndentVp: 0,
        viewportWidthPx: this.viewportWidth,
        viewportHeightPx: this.viewportHeight,
        contentWidthPx: this.contentWidth,
        contentHeightPx: this.contentHeight,
        systemLayoutLineCount: 0,
        caretRect: null,
        selectionRectCount: 0,
        currentLineMetrics: null,
        imeAttached: imeAttached,
        editorFocused: this.isFocused,
        currentScrollX: scrollX,
        currentScrollY: scrollY,
        leadingWhitespaceCountOfCurrentParagraph: 0,
        layoutSource: 'none',
        usingLayoutManager: false,
        apiLevel: apiLevel,
        lastLayoutError: this.lastLayoutError,
        fallbackCount: this.fallbackCount,
        layoutRevision: 0,
        layoutGeneration: 0,
        layoutCompositionGeneration: 0,
        visualCaretAffinity: 'downstream',
        // Issue #768 评论5835171266 第5项：编辑器挂载状态和时间戳。
        isEditorMounted: true,
        capturedAtMs: Date.now(),
      }
      store.updateSnapshot(emptySnapshot)
      return
    }

    // 有活跃会话：从 snapshot 和当前状态构造完整快照。
    const text: string = snap.text
    const textUtf16Length: number = text.length
    const textUtf8Bytes: number = TextOffsetMapper.utf16ToUtf8(text, textUtf16Length)
    const cursorUtf16Offset: number = TextOffsetMapper.utf8ToUtf16(text, snap.cursorByteOffset)
    const hasComposition: boolean = snap.compositionStart !== null && snap.compositionEnd !== null
    const compositionStartByte: number = snap.compositionStart !== null ? snap.compositionStart : -1
    const compositionEndByte: number = snap.compositionEnd !== null ? snap.compositionEnd : -1

    // 排版设置
    const fontSizeVp: number = snap.fontSize
    const lineSpacingMultiplier: number = snap.lineSpacingMultiplier
    const effectiveLineHeightVp: number = fontSizeVp * lineSpacingMultiplier
    const firstLineIndentEnabled: boolean = snap.autoIndentEnabled
    const firstLineIndentChars: number = snap.autoIndentWidth
    const effectiveFirstLineIndentVp: number = firstLineIndentEnabled ? firstLineIndentChars * fontSizeVp : 0

    // 布局来源/行数 — Issue #768 评论5837824666：
    // 不再直接读 getLayoutManager()（可能拿到上一版布局），改为从缓存字段读取。
    // 缓存字段在 refreshRenderLayoutFromSystem/Fallback 成功后更新。
    const usingLayoutManager: boolean = this.lastLayoutSource === 'system'
    const systemLayoutLineCount: number = this.lastSystemLayoutLineCount

    // 光标矩形
    let caretRect: CaretRectSnapshot | null = null
    if (this.caretRect !== null) {
      caretRect = {
        x: this.caretRect.x,
        y: this.caretRect.y,
        width: this.caretRect.width,
        height: this.caretRect.height,
      }
    }

    // 当前行度量 — Issue #768 评论5837824666：
    // 不再从 renderResult.lines 拼（baseline 固定 0、无 left/topHeight），
    // 改为从缓存字段读取（在 refreshRenderLayoutFromSystem 中从真实 layout.getLineMetrics 获取）。
    const currentLineMetrics: CurrentLineMetricsSnapshot | null = this.lastCurrentLineMetrics

    // 段落前导空白
    const leadingWhitespaceCount: number = this.computeLeadingWhitespaceCount(text, cursorUtf16Offset)

    const fullSnapshot: EditorRuntimeSnapshot = {
      hasActiveSession: true,
      revision: snap.revision,
      generation: snap.generation,
      textUtf8Bytes: textUtf8Bytes,
      textUtf16Length: textUtf16Length,
      cursorByteOffset: snap.cursorByteOffset,
      cursorUtf16Offset: cursorUtf16Offset,
      selectionAnchorByte: snap.selectionAnchor,
      selectionHeadByte: snap.selectionHead,
      hasComposition: hasComposition,
      compositionStartByte: compositionStartByte,
      compositionEndByte: compositionEndByte,
      fontSizeVp: fontSizeVp,
      lineSpacingMultiplier: lineSpacingMultiplier,
      effectiveLineHeightVp: effectiveLineHeightVp,
      firstLineIndentEnabled: firstLineIndentEnabled,
      firstLineIndentChars: firstLineIndentChars,
      effectiveFirstLineIndentVp: effectiveFirstLineIndentVp,
      viewportWidthPx: this.viewportWidth,
      viewportHeightPx: this.viewportHeight,
      contentWidthPx: this.contentWidth,
      contentHeightPx: this.contentHeight,
      systemLayoutLineCount: systemLayoutLineCount,
      caretRect: caretRect,
      selectionRectCount: this.selectionRects.length,
      currentLineMetrics: currentLineMetrics,
      imeAttached: imeAttached,
      editorFocused: this.isFocused,
      currentScrollX: scrollX,
      currentScrollY: scrollY,
      leadingWhitespaceCountOfCurrentParagraph: leadingWhitespaceCount,
      layoutSource: this.lastLayoutSource,
      usingLayoutManager: usingLayoutManager,
      apiLevel: apiLevel,
      lastLayoutError: this.lastLayoutError,
      fallbackCount: this.fallbackCount,
      // Issue #768 评论5837824666：布局身份字段。
      layoutRevision: this.layoutRevision,
      layoutGeneration: this.layoutGeneration,
      layoutCompositionGeneration: this.layoutCompositionGeneration,
      visualCaretAffinity: this.visualCaretAffinity,
      // Issue #768 评论5835171266 第5项：编辑器挂载状态和时间戳。
      isEditorMounted: true,
      capturedAtMs: Date.now(),
    }
    store.updateSnapshot(fullSnapshot)
  }

  // Issue #768：计算光标所在段落的前导空白字符数（空格/制表符）。
  private computeLeadingWhitespaceCount(text: string, cursorUtf16: number): number {
    // 找光标所在段落的起点：从 cursor 往前找到 \n 后一个字符或 0
    let paragraphStart: number = cursorUtf16
    while (paragraphStart > 0) {
      if (text.charAt(paragraphStart - 1) === '\n') {
        break
      }
      paragraphStart--
    }
    // 从段落起点数空白字符
    let count: number = 0
    for (let i = paragraphStart; i < text.length; i++) {
      const ch: string = text.charAt(i)
      if (ch === ' ' || ch === '\t') {
        count++
      } else {
        break
      }
    }
    return count
  }

  // Issue #879：@Watch 回调，监听 presentationUpdate 变化。
  // 从 presentationUpdate.layoutSnapshot 同步 currentSnapshot，
  // 并通过 motionCoordinator.acceptUpdate() 更新 visualFrame。
  // Issue #879 复核评论修复（问题1/3/4）：前后台排版隔离——
  // - 不再设置 staticClipCommands='' 隐藏主 Text（会导致闪屏）
  // - 不立即调 refreshRender()——保持主 Text 显示旧内容
  // - 新 StyledString 发给 EditorPendingLayoutHost 排版，不发给主 Text
  // - 待后台布局完成（onLayoutReady 回调），按目标 token 消费并构造完整的新运动计划
  // - 同一次发布新静态可见区、运动源窗口和光标
  // - 交棒后才更新前台 Text
  private onPresentationUpdateChanged(): void {
    // Issue #879 复核评论修复（问题3）：用 displayed 版本缓存旧布局/旧 snapshot/旧 StyledString——
    // previousLayout/previousSnapshot/oldStyledString 只在 displayed 版本切换时更新，
    // 不被 pending 版本覆盖。
    this.previousLayout = this.displayedLineLayouts
    this.previousSnapshot = this.displayedSnapshot
    this.oldStyledString = this.displayedStyledString

    if (this.presentationUpdate === null) {
      this.currentSnapshot = null
      this.displayedPresentation = null
      // Issue #879 复核评论修复（问题3）：清空 displayed 版本字段。
      this.displayedSnapshot = null
      this.displayedLineLayouts = []
      this.displayedStyledString = null
      // Issue #879 本轮复核评论修复（问题6）：会话失效时停止旧动画。
      // 先增 generation 使旧 animator 回调失效，再 cancel。
      this.stopActiveAnimation()
      // Issue #768 评论5837229456：通过 applyStyledText 清空 StyledString（受 textMounted 守卫）
      this.applyStyledText()
      this.lineResolver.cancelWait()
      this.lineResolver.updateLayout(null)
      this.renderResult = null
      this.selectionRects = []
      this.caretRect = null
      this.compositionUnderlineRects = []
      this.displayedVisualFrame = null
      this.candidateVisualFrame = null
      // Issue #879 最新复核评论问题4：会话失效时丢弃候选句柄。
      this.pendingCandidate = null
      // Issue #879 复核评论修复（问题1）：清空候选 token。
      this.candidateToken = null
      this.pendingPresentationUpdates = []
      this.previousLayout = []
      // Issue #879 复核评论修复（问题3）：会话切换时清空 styledStringByToken、consumedTokens、pendingLayoutRequest。
      this.styledStringByToken.clear()
      this.consumedTokens.clear()
      this.pendingLayoutRequest = null
      // Issue #879 复核评论修复（问题3）：清空运动源池。
      this.motionSourcePool.clear()
      // Issue #879 复核评论6077187962 问题4：清空窗口实例 ID 映射。
      this.motionCoordinator.clearWindowInstances()
      // Issue #879 复核评论问题5：清空源排版记录缓存——防止跨会话 revision 重复时取到旧布局。
      this.motionCoordinator.clearSourceLayoutRecords()
      // Issue #879 最新复核评论修复（问题3）：清理窗口级 ComponentObserver 和回调。
      this.cleanupAllWindowsObservers()
      this.pendingFirstCaretRect = null
      // Issue #879 本轮复核评论修复（问题4）：会话切换时清空 pendingDisplayData。
      this.pendingDisplayData = null
      // Issue #879 复核评论修复（问题4）：会话失效时更新裁切 path 为全尺寸矩形。
      this.updateStaticClipCommands()
      // Issue #768：会话失效时推送诊断快照（hasActiveSession=false）。
      this.pushDiagnosticsSnapshot()
      return
    }
    // Issue #879 最新复核评论问题5：会话切换时丢弃上一会话的待发布事务链和候选——
    // 旧会话的排版回调（sessionId 不同）不得被新会话复用。
    if (this.pendingPresentationUpdates.length > 0) {
      const pendingSessionId = this.pendingPresentationUpdates[0].sessionId
      if (pendingSessionId !== this.presentationUpdate.sessionId) {
        this.pendingPresentationUpdates = []
        this.pendingCandidate = null
        this.candidateVisualFrame = null
        this.pendingDisplayData = null
        this.candidateToken = null
        // Issue #879 复核评论问题5：跨会话切换时清空源排版记录缓存。
        this.motionCoordinator.clearSourceLayoutRecords()
      }
    }

    // Issue #879 复核评论修复（问题1）：同会话内收到新 presentationUpdate 时，
    // 立刻使旧候选失效——丢弃 pendingCandidate/pendingDisplayData 与候选自己的等待状态
    // 与旧候选 token，并释放它独占的 staging 窗口；
    // 但保留 pendingPresentationUpdates 事务链（从 displayed revision 开始的全部事务）。
    // 旧候选的窗口布局回调不再携带 token；候选帧被丢弃后它对不上任何窗口，
    // tryCommitCandidate 也会因 candidateToken/pendingDisplayData 为空而拒绝提交。
    if (this.pendingCandidate !== null && this.candidateToken !== null) {
      this.pendingCandidate = null
      this.candidateVisualFrame = null
      this.pendingDisplayData = null
      // 旧候选 token 一并作废——只有 onPendingLayoutReady 为最新 revision 准备出的候选
      // 才能重新拿到 token，旧回调永远对不上。
      this.candidateToken = null
    }

    // Issue #879 复核评论修复（问题3）：不立即切换 currentSnapshot——
    // 保持 currentSnapshot 指向 displayed 版本（旧内容），新内容暂存到 pending 字段。
    // currentSnapshot 在 onPendingLayoutReady 中才更新为新版本。

    // Issue #879 复核评论修复（问题2）：立即把 latestCoreSnapshot 更新为 Core 最新 layoutSnapshot。
    // 不等后台布局完成或显示提交——Core 已到这个 revision，位置映射的目标必须立刻跟上。
    // currentSnapshot/displayedSnapshot 仍延到显示提交时才更新（它们对应主 Text 的 LayoutManager）。
    this.latestCoreSnapshot = this.presentationUpdate.layoutSnapshot

    // Issue #879 复核评论修复（问题4）：缓存 pending 身份字段。
    // layout observer 回调中校验 pending 的 revision/generation/composition/style/width 匹配后才消费。
    const pendingSnap = this.presentationUpdate.layoutSnapshot
    this.pendingRevision = pendingSnap.revision
    this.pendingGeneration = pendingSnap.generation
    this.pendingCompositionGeneration = pendingSnap.compositionGeneration
    this.pendingFontSize = pendingSnap.fontSize
    this.pendingLineSpacing = pendingSnap.lineSpacingMultiplier
    this.pendingContentWidth = this.contentWidth

    // Issue #879 复核评论修复（问题3）：构造新 StyledString 用 pendingSnap（新内容），
    // 而非 this.currentSnapshot（旧内容）。
    const config: EditorStyleConfig = {
      fontSize: pendingSnap.fontSize,
      lineSpacingMultiplier: pendingSnap.lineSpacingMultiplier,
      autoIndentEnabled: pendingSnap.autoIndentEnabled,
      autoIndentWidth: pendingSnap.autoIndentWidth,
    }
    this.newStyledString = EditorTextStyleProjector.projectToStyledString(
      pendingSnap.text, config
    )

    // Issue #879 复核评论修复（问题1）：不再设置 staticClipCommands='' 隐藏主 Text。
    // 主 Text 保持显示旧内容（上一帧的全部文字、裁切窗口和唯一光标），
    // 直到后台排版完成后才更新前台 Text。
    // 也不立即调 refreshRender()——新 StyledString 只发给 EditorPendingLayoutHost 排版。

    // Issue #879 最新复核评论修复（问题1）：追加到事务链数组末尾，而非覆盖。
    // 连续快速输入时，多笔事务按顺序保存，onPendingLayoutReady 中合并消费。
    this.pendingPresentationUpdates.push(this.presentationUpdate)

    // Issue #879 复核评论修复（问题1）：通过 EditorPendingLayoutHost 请求后台排版新 StyledString。
    // 新 StyledString 不发给主 Text——主 Text 继续显示旧内容。
    // 后台排版完成后（onPendingLayoutReady 回调），才更新前台 Text 并构造运动计划。
    // ArkUI 中 @Component struct 的方法不能从父组件直接调用，
    // 所以通过 @State pendingLayoutRequest + @Prop + @Watch 模式触发排版。
    // Issue #879 复核评论修复（问题1）：PendingLayoutRequest 只含基本类型——StyledString 不走 @Prop 深拷贝，
    // 父组件维护 token→StyledString 映射，EditorPendingLayoutHost 通过 resolveLayoutRequest 回调获取引用。
    if (this.newStyledString !== null && this.contentWidth > 0) {
      this.pendingLayoutToken++
      // 把 StyledString 存入 token 映射——EditorPendingLayoutHost 通过回调获取
      this.styledStringByToken.set(this.pendingLayoutToken, this.newStyledString)
      // Issue #879 复核评论修复（问题3）：新请求到来时清理所有不再等待的旧 token→StyledString 映射条目。
      // 两次后台排版之间若 token 1 被 token 2 替换，token 1 从未被清理——这里统一清理。
      const keysToDelete: number[] = []
      this.styledStringByToken.forEach((_: StyledString, key: number) => {
        if (key < this.pendingLayoutToken) {
          keysToDelete.push(key)
        }
      })
      for (const key of keysToDelete) {
        this.styledStringByToken.delete(key)
      }
      const fullTextLength = this.newStyledString.getString().length
      // Issue #879 复核评论修复（问题3）：用 pendingSnap 的 fontSize/lineSpacing，
      // 而非 this.currentSnapshot（旧内容）的值。
      const fontSize = pendingSnap.fontSize
      const lineSpacing = pendingSnap.lineSpacingMultiplier
      // Issue #879 复核评论修复（问题2）：styleKey 包含更多影响几何的值——
      // 至少包含 fontSize、lineSpacing、autoIndentEnabled、autoIndentWidth。
      const autoIndentEnabled = pendingSnap.autoIndentEnabled
      const autoIndentWidth = pendingSnap.autoIndentWidth
      // Issue #879 最新复核评论修复（问题5）：用完整文本内容替代 textHash。
      // textHash 只有 length:firstCharCode:lastCharCode，"甲中乙"和"甲文乙"相同。
      // fullText 是完整文本内容，相同长度/首尾字符也能区分。
      const fullText = this.newStyledString.getString()
      // Issue #879 最新复核评论修复（问题5）：使用真实 Core 信息——
      // sessionId 用 presentationUpdate.sessionId（Core 编辑会话 ID），
      // revision 用 pendingSnap.revision（layoutSnapshot.revision）。
      // 不再用 editorSessionId（Date.now()）或 pendingLayoutToken。
      const sessionId = this.presentationUpdate.sessionId
      const revision = pendingSnap.revision
      const request: PendingLayoutRequest = {
        token: this.pendingLayoutToken,
        contentWidth: this.contentWidth,
        fontSize: fontSize,
        fullTextLength: fullTextLength,
        styleKey: `${fontSize}-${lineSpacing}-${autoIndentEnabled}-${autoIndentWidth}`,
        fullText: fullText,
        sessionId: sessionId,
        revision: revision,
      }
      this.pendingLayoutRequest = request
    }

    // Issue #768 评论5837824666：推送诊断快照（布局相关字段从缓存读取）。
    this.pushDiagnosticsSnapshot()
  }

  // Issue #879 复核评论修复（问题1）：EditorPendingLayoutHost 排版完成回调。
  // 后台排版完成后，安全地更新前台 Text 并构造运动计划。
  // - 校验 token 匹配——防止旧请求的回调覆盖新请求
  // - 把新 StyledString 写入主 Text（触发主 Text 的 layout）
  // - 用后台排版结果（lineLayouts）作为新布局，内联调 acceptUpdateWithChain
  // - 不用等主 Text 的 layout observer 回调——后台排版结果已经是正确的行布局
  //
  // Issue #879 最新复核评论修复（问题1）：消费整个事务链——
  // 从 pendingPresentationUpdates 数组中取出所有事务，收集所有 editResult，
  // 调用 motionCoordinator.acceptUpdateWithChain 合并事务链。
  // Issue #879 最新复核评论修复（问题5）：增加可选 firstCaretRect 参数。
  // 空文本时 lineLayouts 为空数组，firstCaretRect 提供光标位置（含 autoIndent/padding）。
  //
  // Issue #879 本轮复核评论修复（问题4）：两阶段发布——
  // 运动窗口 layoutReady 之前不切换主 Text 内容和静态遮罩。
  // 1. 生成候选 plan（调 prepareCandidate），设置 visualFrame（让运动窗口 ForEach 创建组件）
  // 2. 准备运动源（prepareMotionSources）
  // 3. 保存 committedLineLayouts
  // 4. 不更新 displayedSnapshot/displayedLineLayouts/displayedStyledString
  // 5. 不调 refreshRender()（主 Text 保持旧内容）
  // 6. 不调 updateStaticClipCommands()（保持全尺寸矩形，不裁切）
  // 7. 暂存 pendingDisplayData
  // 8. 如果有动画且窗口已 ready → 立即执行显示切换 + 启动动画
  // 9. 如果有动画但窗口未 ready → 等待属于本候选的窗口 layout 回调再提交（时长在 token 上）
  // 10. 如果无动画 → 立即执行显示切换
  //
  // Issue #879 复核评论修复（问题3）：本阶段不停旧 animator——保持旧 animator 运行。
  // 旧 animator 继续按已提交 plan 驱动 displayedVisualFrame，屏幕上的上一笔吐字/光标动画
  // 不会在候选准备阶段停顿。只有 executeDisplaySwitch（窗口 ready 后才调）才停旧 animator、
  // 冻结真实屏幕帧并用 retargetCandidate 把候选起点对齐到该帧。
  //
  // Issue #879 复核评论修复（问题1）：创建 candidateToken 标识本次候选身份；
  // 提交由 tryCommitCandidate 兜底（窗口 layout 回调与候选准备流程共用同一入口）。
  private onPendingLayoutReady(token: number, lineLayouts: LineLayout[], firstCaretRect?: CaretRect): void {
    // Issue #879 复核评论修复（问题3）：要求 pendingPresentationUpdates 非空才消费。
    if (token !== this.pendingLayoutToken || this.pendingPresentationUpdates.length === 0) {
      return
    }
    // Issue #879 复核评论修复（问题3）：检查已消费的 token——防止同一 token 重复消费。
    if (this.consumedTokens.has(token)) {
      return
    }
    this.consumedTokens.add(token)
    // Issue #879 最新复核评论修复（问题5）：保存 firstCaretRect——
    // 空正文时由 EditorPendingLayoutHost 从 LayoutManager 计算的光标位置，
    // 传递给 Coordinator 的 acceptUpdateWithChain，空正文时优先使用而非估算。
    this.pendingFirstCaretRect = firstCaretRect ?? null

    // Issue #879 复核评论修复（问题3）：本阶段不停旧 animator——
    // 旧 animator 继续按已提交 plan 驱动 displayedVisualFrame，
    // 屏幕上的上一笔吐字/光标动画不会在候选准备阶段停顿。
    // 只有 executeDisplaySwitch（窗口 ready 后才调）才停旧 animator 并冻结屏幕帧。

    // Issue #879 本轮复核评论修复（问题4）：步骤2——取最新版本，生成候选 plan。
    const latestUpdate = this.pendingPresentationUpdates[this.pendingPresentationUpdates.length - 1]
    const oldSnapshot = this.previousSnapshot ?? this.displayedSnapshot ?? latestUpdate.layoutSnapshot

    // 收集所有 pendingPresentationUpdates 的 editResult 形成事务链
    const editResults: EditorEditResult[] = []
    for (const update of this.pendingPresentationUpdates) {
      if (update.editResult !== null) {
        editResults.push(update.editResult)
      }
    }

    // Issue #879 最新复核评论修复（问题2）：步骤3——保存 Core 最新编辑事实到 latestCoreSnapshot，
    // 不再提前更新 currentSnapshot——currentSnapshot 只在 executeDisplaySwitch 中与 displayedSnapshot 一起更新。
    // 后台候选等待期间，hitTest/cursor 使用 displayedSnapshot（对应主 Text 的 LayoutManager）。
    this.latestCoreSnapshot = latestUpdate.layoutSnapshot

    // Issue #879 本轮复核评论修复（问题4）：步骤4——保存后台排版的行布局。
    this.committedLineLayouts = lineLayouts

    // Issue #879 最新复核评论修复（问题4）：步骤5——使用 prepareCandidate 生成候选 plan，
    // 不修改 Coordinator 的 currentPlan/currentFrame——旧动画在交棒期间仍能正确使用旧状态。
    // displayedVisualFrame 作为起始状态传给 prepareCandidate。
    const displayedFrameForCandidate = this.displayedVisualFrame ?? this.motionCoordinator.getCurrentFrame()
    const prepared = this.motionCoordinator.prepareCandidate(
      oldSnapshot,
      this.previousLayout,
      this.latestCoreSnapshot ?? this.currentSnapshot,
      lineLayouts,
      editResults,
      this.contentWidth,
      this.motionPolicy,
      this.pendingFirstCaretRect,
      displayedFrameForCandidate,
      latestUpdate.sessionId
    )
    // 旧候选（若有）被本次更高 revision 的候选取代：直接丢弃即可，
    // Coordinator 的已提交计划/帧从未被候选改动，旧动画仍按旧 plan 采样。
    this.pendingCandidate = prepared
    this.candidateVisualFrame = prepared.startFrame

    // Issue #879 复核评论修复（问题1）：创建 candidateToken 标识本次候选身份，
    // 提交前由 executeDisplaySwitch 核对 generation/session/revision/layoutToken。
    this.candidateGeneration++
    // Issue #879 复核评论6075662695问题2：等启动时长挂在候选自己身上（token.durationMs），
    // 不再写全局 pendingAnimationDurationMs——旧窗口回调无法吞掉新候选的启动时长。
    // 注意：prepareCandidate 不修改 Coordinator 的 currentPlan，所以不能用
    // motionCoordinator.isAnimating()/getCurrentPlan().durationMs 判断候选动画状态，
    // 只能用 prepared.plan.durationMs。
    this.candidateToken = {
      sessionId: latestUpdate.sessionId,
      revision: latestUpdate.layoutSnapshot.revision,
      layoutToken: token,
      generation: this.candidateGeneration,
      durationMs: prepared.plan.durationMs
    }

    // Issue #879 本轮复核评论修复（问题4）：步骤6——准备运动源。
    this.prepareMotionSources()

    // Issue #879 本轮复核评论修复（问题4）：步骤7——暂存 pendingDisplayData。
    // 不更新 displayedSnapshot/displayedLineLayouts/displayedStyledString，
    // 不调 refreshRender()（主 Text 保持旧内容），
    // 不调 updateStaticClipCommands()（保持全尺寸矩形，不裁切）。
    this.pendingDisplayData = {
      latestUpdate: latestUpdate,
      lineLayouts: lineLayouts,
      newStyledString: this.newStyledString,
    } as PendingDisplayData

    // Issue #879 复核评论6075662695问题2：步骤8——所有运动窗口都已就绪就立刻提交，
    // 否则什么都不做：等待属于本候选的窗口 layout 回调再调 tryCommitCandidate。
    // 未就绪的窗口一定是本候选新建的 renderNodeKey，它的回调会带上自己的 key，
    // 由 isWindowOfCurrentCandidate 判定归属——不依赖「最后一个无关窗口」的布局回调。
    this.tryCommitCandidate(null)

    // Issue #879 复核评论修复（问题1）：清理已消费的 token→StyledString 映射条目
    this.styledStringByToken.delete(token)
  }

  // Issue #879 复核评论修复（问题1/4）：两阶段发布——执行显示切换 + 原子提交候选帧。
  // 在运动窗口 layoutReady 之后（或无动画时），统一切换主 Text 内容、静态遮罩、
  // displayed 版本字段和 currentSnapshot。
  // 从 pendingDisplayData 取出暂存的数据，更新所有 displayed 版本字段，
  // Issue #879 复核评论修复（问题1/3/4）：两阶段发布——执行显示切换 + 原子提交候选帧。
  // 在运动窗口 layoutReady 之后（或无动画时），统一切换主 Text 内容、静态遮罩、
  // displayed 版本字段和 currentSnapshot。
  // 从 pendingDisplayData 取出暂存的数据，更新所有 displayed 版本字段，
  // 写入新 StyledString 到主 Text，更新静态裁切 path。
  // Issue #879 最新复核评论修复（问题4）：调用 commitCandidate 一次性替换 Coordinator 的内部状态。
  // Issue #879 复核评论修复（问题1）：原子提交候选帧——
  // displayedVisualFrame = candidateVisualFrame，candidateVisualFrame = null。
  // 提交后 ForEach 中原候选窗口 visible 从 false 变为 true（因为现在它们属于 displayedVisualFrame），
  // 旧 displayed 窗口被移除（renderNodeKey 不同）。
  //
  // Issue #879 复核评论修复（问题1）：executeDisplaySwitch(expectedToken)——提交前再次核对身份，
  // 只有确属当前候选的 token 才能消费事务链；旧候选的回调拿不到有效 token。
  // 消费只清掉截至已提交 revision 的事务，不能无条件 pendingPresentationUpdates=[]。
  //
  // Issue #879 复核评论修复（问题3）：窗口 ready 后才调本方法——
  // 先 ++motionGeneration + cancel 旧 animator → 冻结当前 displayedVisualFrame →
  // 调 retargetCandidate 重新规划候选 → commitCandidate → 更新主 Text。
  // 旧 animator 在候选准备阶段继续运行，屏幕上的上一笔吐字/光标动画不停顿；
  // 提交瞬间才停止旧 animator，冻结真实屏幕帧，用 retargetCandidate 把候选 plan 的
  // 起始 clip/position 替换为冻结帧的真实值，再一次性 commitCandidate 和更新主 Text。
  //
  // @returns 真正提交了返回 true；token 已过期或没有候选数据时返回 false（调用方不得再启动动画）。
  private executeDisplaySwitch(expectedToken: CandidateToken | null): boolean {
    // Issue #879 复核评论修复（问题1）：提交前再次核对身份——
    // 必须是「当前有效候选」的 token（generation/session/revision 全同）。
    // 旧候选的窗口布局回调携带旧 candidateToken：候选已被更高 revision 作废时
    // candidateToken 为 null 或 generation 不同，回调无权消费新链。
    if (expectedToken === null || this.candidateToken === null) {
      return false
    }
    if (expectedToken.generation !== this.candidateToken.generation
      || expectedToken.sessionId !== this.candidateToken.sessionId
      || expectedToken.revision !== this.candidateToken.revision
      || expectedToken.layoutToken !== this.candidateToken.layoutToken) {
      return false
    }
    if (this.pendingDisplayData === null) {
      return false
    }
    const data = this.pendingDisplayData
    this.pendingDisplayData = null

    // Issue #879 复核评论修复（问题3）：先 ++motionGeneration + cancel 旧 animator → 冻结当前 displayedVisualFrame。
    // 旧 animator 在候选准备阶段继续运行，提交瞬间才停止。
    // 必须先 ++motionGeneration 再 cancel——cancel 会额外触发一次终点 onFrame，
    // 先 cancel 再 ++generation 会让旧动画终值写到新画面。
    this.motionGeneration++
    if (this.motionAnimator !== null) {
      this.motionAnimator.cancel()
      this.motionAnimator = null
    }
    this.isMotionAnimating = false

    // 冻结当前 displayedVisualFrame——这是旧 animator 停止后的真实屏幕状态。
    // 其 MotionGlyphWindow.currentClipLeft/Right/currentPositionX/Y 反映真实屏幕进度。
    const frozenDisplayedFrame = this.displayedVisualFrame ?? this.motionCoordinator.getCurrentFrame()
    // Issue #879 复核评论6075662695问题3：提交瞬间的「在屏」上下文——
    // 必须在覆盖 displayed* 字段之前取出，retargetCandidate 用它按字形重算起始 clip/position。
    // 身份表用 Coordinator 当前已提交版本的表（描述在屏正文，提交前不推进）。
    const frozenDisplayed: DisplayedSnapshotContext = {
      snapshot: this.displayedSnapshot ?? data.latestUpdate.layoutSnapshot,
      layout: this.displayedLineLayouts,
      identities: this.motionCoordinator.getCommittedIdentities()
    }

    // 更新 displayed 版本字段
    this.displayedSnapshot = data.latestUpdate.layoutSnapshot
    this.displayedLineLayouts = data.lineLayouts
    this.displayedStyledString = data.newStyledString
    this.displayedPresentation = data.latestUpdate

    // Issue #879 最新复核评论修复（问题2）：currentSnapshot 在这里与 displayedSnapshot 一起更新——
    // 不再在 onPendingLayoutReady 中提前更新。后台候选等待期间 hitTest/cursor 使用 displayedSnapshot。
    this.currentSnapshot = data.latestUpdate.layoutSnapshot
    this.latestCoreSnapshot = data.latestUpdate.layoutSnapshot

    // Issue #879 复核评论修复（问题3）：调 retargetCandidate 重新规划候选 → commitCandidate → 更新主 Text。
    // 用提交瞬间真实在屏布局 + 冻结帧按字形重算起始 clip/position，
    // 确保新动画从真实屏幕状态开始，而非从过时的起始状态跳变。
    const candidate = this.pendingCandidate
    if (candidate !== null) {
      this.pendingCandidate = null
      const retargeted = this.motionCoordinator.retargetCandidate(
        candidate, frozenDisplayedFrame, frozenDisplayed
      )
      this.displayedVisualFrame = this.motionCoordinator.commitCandidate(retargeted)
      this.candidateVisualFrame = null
    }

    // 写入新 StyledString 到主 Text
    this.refreshRender()

    // 更新静态裁切 path
    this.updateStaticClipCommands()

    // Issue #879 复核评论修复（问题1）：消费只清掉截至已提交 revision 的事务——
    // 不能无条件 pendingPresentationUpdates=[]。
    // 保留 revision > committedRevision 的更新（如 revision 12 的事务），
    // 等其后台排版完成后 onPendingLayoutReady 消费。
    const committedRevision = data.latestUpdate.layoutSnapshot.revision
    const remainingUpdates: EditorPresentationUpdate[] = []
    for (const update of this.pendingPresentationUpdates) {
      if (update.layoutSnapshot.revision > committedRevision) {
        remainingUpdates.push(update)
      }
    }
    this.pendingPresentationUpdates = remainingUpdates

    // Issue #879 复核评论修复（问题1）：只要还留着未消费的更新，链的基准就变成了刚提交的
    // displayed 版本——previous* 必须跟着前进，否则下一次候选会用过时的基准拼链。
    if (remainingUpdates.length > 0) {
      this.previousSnapshot = this.displayedSnapshot
      this.previousLayout = this.displayedLineLayouts
      this.oldStyledString = this.displayedStyledString
    }

    // Issue #879 复核评论修复（问题1）：提交后清空 candidateToken——
    // 下次 onPendingLayoutReady 会创建新 token。
    this.candidateToken = null
    return true
  }

  // Issue #879 复核评论修复（问题3）：用 EditorMotionSourcePool 替代旧的 controller 数组。
  // 源池用 sourceRevision + stableRunId 保存 StyledString/TextController，
  // ForEach key 基于稳定 run 身份（runId），而非一次性自增事务编号。
  // Issue #879 复核评论修复（问题4）：池所有权绑定到当前显示帧实际使用的源——
  // 不再被任何窗口引用且不属于 displayed/pending 版本的源立即标记释放并清理。
  // Issue #879 复核评论修复（问题1）：使用 candidateVisualFrame 准备运动源——
  // candidateVisualFrame 是后台准备的候选帧，其窗口需要创建源并参与布局。
  // Issue #879 最新复核评论修复（问题1）：存活根集合包含 displayedVisualFrame 和
  // candidateVisualFrame 的所有 runId——两阶段交棒期间 displayedVisualFrame 仍可见，
  // 其中可能有不同版本的 old-... 源（如 old-10），不能因候选只含 old-11/new-12 就释放 old-10。
  // Issue #879 最新复核评论问题1：存活根集合 = displayed ∪ candidate ∪ 当前主文本源。
  // 两阶段发布期间 displayed 帧仍可见（主 Text 还是旧内容），它的窗口 runId
  // 也可能是不同版本（如 old-10），不能因为候选只含 old-11/new-12 就释放。
  // 主文本源（主 Text 正在渲染的 StyledString）由 displayed/candidate 的 revision 兜底保护。
  private collectLiveSourceRoots(): MotionSourceRoots {
    const roots: MotionSourceRoots = {
      runIds: new Set<string>(),
      revisions: new Set<number>()
    }
    const frames: (EditorVisualFrame | null)[] = [this.displayedVisualFrame, this.candidateVisualFrame]
    for (const frame of frames) {
      if (frame === null || !frame.isAnimating) {
        continue
      }
      const windows = frame.motionGlyphWindows
      for (let i = 0; i < windows.length; i++) {
        roots.runIds.add(windows[i].runId)
        roots.revisions.add(windows[i].sourceRevision)
      }
    }
    // 主文本源：displayed 版本是主 Text 正在渲染的内容；
    // candidate 版本是本次将要接管的新主文本源，两者都必须存活。
    if (this.displayedSnapshot !== null) {
      roots.revisions.add(this.displayedSnapshot.revision)
    }
    if (this.latestCoreSnapshot !== null) {
      roots.revisions.add(this.latestCoreSnapshot.revision)
    }
    const oldRevision = this.previousSnapshot?.revision
    if (oldRevision !== undefined) {
      roots.revisions.add(oldRevision)
    }
    return roots
  }

  private prepareMotionSources(): void {
    if (this.candidateVisualFrame === null || !this.candidateVisualFrame.isAnimating) {
      return
    }
    this.motionSourcePool.releaseUnusedSources(this.collectLiveSourceRoots())

    // Issue #879 最新复核评论修复（问题3）：窗口级状态（TextController/mounted/layoutReady）
    // 已移到 MotionWindowState 中管理，不再需要手动清理 motionWindowControllers。
    // 窗口状态的创建和释放由 getOrCreateWindowState/releaseWindowState 自动管理。

    // 为运动窗口获取或创建源
    const oldRevision = this.previousSnapshot?.revision ?? 0
    // Issue #879 最新复核评论修复（问题2）：使用 latestCoreSnapshot 获取新版本 revision——
    // currentSnapshot 在 executeDisplaySwitch 之前还是旧版本。
    const newRevision = this.latestCoreSnapshot?.revision ?? this.currentSnapshot?.revision ?? 0

    if (this.oldStyledString !== null) {
      this.motionSourcePool.getOrCreateSource('old', oldRevision, this.oldStyledString)
    }
    if (this.newStyledString !== null) {
      this.motionSourcePool.getOrCreateSource('new', newRevision, this.newStyledString)
    }
  }

  // 预匹配运动窗口与源——在 build() 之前调用，避免在 @Builder 中做匹配（ArkUI 限制）。
  // ForEach key 基于 window.renderNodeKey（稳定身份），而非 `${runId}-${index}`（数组序号）。
  // Issue #879 最新复核评论修复（问题3）：
  // - 从 motionSourcePool.getOrCreateWindowState(renderNodeKey) 读取 mounted/layoutReady/textController，
  //   而非从 source.mounted/source.layoutReady 读取（这些字段已从 MotionSourceEntry 移除）。
  // - 设置 onWindowLayoutReady 和 onWindowDisappear 回调：
  //   onWindowLayoutReady(renderNodeKey) → SujianEditor 创建 ComponentObserver 监听 layout 事件，
  //     layout 回调中调用 markWindowLayoutReady + markWindowMounted。
  //   onWindowDisappear(renderNodeKey) → SujianEditor 清理 ComponentObserver 和窗口状态。
  // - TextController 从 windowState.textController 获取（而非从 motionWindowControllers Map）。
  // Issue #879 复核评论修复（问题1）：buildMotionWindowSources 构建可见窗口源（visible=true），
  // buildCandidateMotionWindowSources 构建候选窗口源（visible=false，opacity 0 参与布局）。
  // 两者合并后传给 MotionTextOverlayBuilder 的同一 ForEach——候选窗口 ready 后原子提交时
  // visible 从 false 变为 true，ForEach 不重建组件（key 不变），只更新 opacity。
  private buildMotionWindowSources(): MotionWindowSource[] {
    if (this.displayedVisualFrame === null || !this.displayedVisualFrame.isAnimating) {
      return []
    }
    const clipRects = EditorMotionLayer.buildAllClipRects(this.displayedVisualFrame)
    const result: MotionWindowSource[] = []
    const windows = this.displayedVisualFrame.motionGlyphWindows
    for (let i = 0; i < windows.length; i++) {
      const source = this.motionSourcePool.getSource(windows[i].runId)
      if (source !== undefined) {
        const renderNodeKey = windows[i].renderNodeKey
        const windowState = this.motionSourcePool.getOrCreateWindowState(renderNodeKey)
        result.push({
          window: windows[i],
          source: source,
          clipRect: clipRects[i],
          textController: windowState.textController,
          mounted: windowState.mounted,
          layoutReady: windowState.layoutReady,
          visible: true,
          onWindowLayoutReady: (rnKey: string): void => {
            this.attachWindowLayoutObserver(rnKey)
          },
          onWindowAcquireLease: (rnKey: string, winInstanceId: string): void => {
            // Issue #879 复核评论问题4：lease 在 onAppear 后才 acquire——
            // buildFrame 不再登记 lease，只有 Text 真正挂载后才 acquire。
            this.motionCoordinator.acquireRenderNodeLease(rnKey, winInstanceId)
          },
          onWindowDisappear: (rnKey: string, winInstanceId: string): void => {
            this.detachWindowLayoutObserver(rnKey)
            this.motionSourcePool.releaseWindowState(rnKey)
            // Issue #879 复核评论6078682695 问题3：按 renderNodeKey 释放租约，
            // 同一 instanceId 被多个节点共享时只有最后一个 onDisAppear 才真正释放。
            this.motionCoordinator.releaseRenderNodeLease(rnKey, winInstanceId)
          }
        })
      }
    }
    return result
  }

  // Issue #879 复核评论修复（问题1）：构建候选窗口源（visible=false，opacity 0）。
  // 候选窗口在同一 ForEach 中渲染，参与布局但不可见。
  // 候选窗口 layoutReady 后原子提交时 visible 变为 true。
  private buildCandidateMotionWindowSources(): MotionWindowSource[] {
    if (this.candidateVisualFrame === null || !this.candidateVisualFrame.isAnimating) {
      return []
    }
    const clipRects = EditorMotionLayer.buildAllClipRects(this.candidateVisualFrame)
    const result: MotionWindowSource[] = []
    const windows = this.candidateVisualFrame.motionGlyphWindows
    for (let i = 0; i < windows.length; i++) {
      const source = this.motionSourcePool.getSource(windows[i].runId)
      if (source !== undefined) {
        const renderNodeKey = windows[i].renderNodeKey
        const windowState = this.motionSourcePool.getOrCreateWindowState(renderNodeKey)
        result.push({
          window: windows[i],
          source: source,
          clipRect: clipRects[i],
          textController: windowState.textController,
          mounted: windowState.mounted,
          layoutReady: windowState.layoutReady,
          visible: false,
          onWindowLayoutReady: (rnKey: string): void => {
            this.attachWindowLayoutObserver(rnKey)
          },
          onWindowAcquireLease: (rnKey: string, winInstanceId: string): void => {
            // Issue #879 复核评论问题4：lease 在 onAppear 后才 acquire——
            // buildFrame 不再登记 lease，只有 Text 真正挂载后才 acquire。
            this.motionCoordinator.acquireRenderNodeLease(rnKey, winInstanceId)
          },
          onWindowDisappear: (rnKey: string, winInstanceId: string): void => {
            this.detachWindowLayoutObserver(rnKey)
            this.motionSourcePool.releaseWindowState(rnKey)
            // Issue #879 复核评论6078682695 问题3：按 renderNodeKey 释放租约，
            // 同一 instanceId 被多个节点共享时只有最后一个 onDisAppear 才真正释放。
            this.motionCoordinator.releaseRenderNodeLease(rnKey, winInstanceId)
          }
        })
      }
    }
    return result
  }

  // Issue #879 复核评论修复（问题1）：合并可见窗口源和候选窗口源。
  // ForEach key 是 renderNodeKey，displayed 和 candidate 的 renderNodeKey 不同
  // （sourceRevision 不同），不会冲突。候选窗口 ready 后原子提交时
  // displayedVisualFrame = candidateVisualFrame，candidateVisualFrame = null，
  // 旧 displayed 窗口被移除，原 candidate 窗口 visible 变为 true。
  private buildAllMotionWindowSources(): MotionWindowSource[] {
    return [...this.buildMotionWindowSources(), ...this.buildCandidateMotionWindowSources()]
  }

  /**
   * Issue #879 复核评论6075662695问题2：提交候选的唯一入口。
   *
   * 等待状态属于候选自己（candidateToken.durationMs），窗口 layout 回调只是触发时机，
   * 不再有「谁先把全局 pendingAnimationDurationMs 清零」的抢占。
   *
   * @param triggeredByKey 触发本次尝试的窗口 renderNodeKey；
   *        null 表示由候选准备流程主动尝试（无窗口回调）。
   * @returns 真正提交并（有动画时）启动动画返回 true。
   */
  private tryCommitCandidate(triggeredByKey: string | null): boolean {
    const token = this.candidateToken
    if (token === null || this.pendingDisplayData === null || this.pendingCandidate === null) {
      return false
    }
    // 归属校验：只有属于当前候选帧的窗口才有权推进提交。
    // 旧 displayed / 旧候选窗口的 layout 回调既不能提交版本，也不能吞掉新候选的启动时长。
    if (triggeredByKey !== null && !this.isWindowOfCurrentCandidate(triggeredByKey)) {
      return false
    }
    if (!this.allMotionWindowsReady()) {
      // 还有窗口没布局完——保持旧画面，等本候选自己的窗口回调再次触发。
      return false
    }
    if (!this.executeDisplaySwitch(token)) {
      return false
    }
    if (token.durationMs > 0) {
      this.startMotionAnimation(token.durationMs)
    }
    return true
  }

  /**
   * Issue #879 复核评论6075662695问题2：判断一个窗口是否属于当前候选帧。
   *
   * 用「窗口所属帧」而不是「attach 时捕获的 token」判定归属：
   * 候选窗口复用同一 renderNodeKey（同 revision 重新准备）时，
   * 观察者仍是旧回调，但它渲染的确实是当前候选的窗口，理应有权推进提交。
   */
  private isWindowOfCurrentCandidate(renderNodeKey: string): boolean {
    const frame = this.candidateVisualFrame
    if (frame === null || !frame.isAnimating) {
      return false
    }
    const windows = frame.motionGlyphWindows
    for (let i = 0; i < windows.length; i++) {
      if (windows[i].renderNodeKey === renderNodeKey) {
        return true
      }
    }
    return false
  }

  // Issue #879 本轮复核评论修复（问题2）：为运动窗口创建 ComponentObserver 监听 layout 事件。
  // @Builder 函数中不能直接创建 ComponentObserver——通过 onWindowLayoutReady 回调让 SujianEditor 创建。
  // ComponentObserver 通过组件 id（`motion-window-${renderNodeKey}`）监听 layout 事件，
  // layout 回调中调用 markWindowLayoutReady + markWindowMounted，确认布局真正完成。
  // renderNodeKey 包含 sourceKind + sourceRevision，切源时创建新 observer 和窗口状态。
  //
  // Issue #879 复核评论6075662695问题2：回调不再携带 attach 时捕获的 token——
  // 改成「标记本窗口 ready + 尝试提交」，由 isWindowOfCurrentCandidate 判定归属。
  private attachWindowLayoutObserver(renderNodeKey: string): void {
    if (this.windowLayoutObservers.has(renderNodeKey)) {
      return
    }
    const componentId = `motion-window-${renderNodeKey}`
    const observer = this.getUIContext()
      .getUIInspector()
      .createComponentObserver(componentId)
    const callback = (): void => {
      this.motionSourcePool.markWindowLayoutReady(renderNodeKey)
      this.motionSourcePool.markWindowMounted(renderNodeKey)
      // 属于当前候选帧的窗口 → 若所有窗口已 ready，立刻提交并启动候选自己的动画时长。
      this.tryCommitCandidate(renderNodeKey)
    }
    observer.on('layout', callback)
    this.windowLayoutObservers.set(renderNodeKey, observer)
    this.windowLayoutCallbacks.set(renderNodeKey, callback)
  }

  // Issue #879 本轮复核评论修复（问题2）：清理运动窗口的 ComponentObserver。
  // Text.onDisAppear 中调用——窗口已从组件树移除，observer 不再需要。
  private detachWindowLayoutObserver(renderNodeKey: string): void {
    const observer = this.windowLayoutObservers.get(renderNodeKey)
    const callback = this.windowLayoutCallbacks.get(renderNodeKey)
    if (observer !== undefined && callback !== undefined) {
      observer.off('layout', callback)
    }
    this.windowLayoutObservers.delete(renderNodeKey)
    this.windowLayoutCallbacks.delete(renderNodeKey)
  }

  // Issue #879 最新复核评论修复（问题3）：清理所有窗口级 ComponentObserver。
  // 会话切换、编辑器销毁时调用——释放所有 observer 和回调。
  private cleanupAllWindowsObservers(): void {
    this.windowLayoutObservers.forEach((observer: inspector.ComponentObserver, renderNodeKey: string) => {
      const callback = this.windowLayoutCallbacks.get(renderNodeKey)
      if (callback !== undefined) {
        observer.off('layout', callback)
      }
    })
    this.windowLayoutObservers.clear()
    this.windowLayoutCallbacks.clear()
  }

  // Issue #879 复核评论修复（问题1）：检查候选帧的运动窗口是否已 layoutReady。
  // 启动动画前检查——未 ready 的窗口保持旧画面显示，等所有窗口 ready 后才启动动画。
  // 使用 candidateVisualFrame 而非 displayedVisualFrame——候选窗口是正在准备的新窗口。
  private allMotionWindowsReady(): boolean {
    if (this.candidateVisualFrame === null || !this.candidateVisualFrame.isAnimating) {
      return true
    }
    const windows = this.candidateVisualFrame.motionGlyphWindows
    for (let i = 0; i < windows.length; i++) {
      if (!this.motionSourcePool.isWindowLayoutReady(windows[i].renderNodeKey)) {
        return false
      }
    }
    return true
  }

  // Issue #879 本轮复核评论修复（问题4）：校验 pending RenderPlan 身份。
  // layout observer 回调中核实 sessionId/revision/generation/composition/style/width 与 pending 一致。
  // 需保留 per-revision 的旧布局，不要在两次更新尚未消费之间用 previousLayout 一个变量覆盖旧版来源。
  // Issue #879 本轮复核评论修复（问题4 增强）：用 LayoutManager 校验排版内容确实对应 pending revision。
  // 旧实现只比 currentSnapshot 和 pendingRevision/generation/style/width——它们当然相等（同一份数据）。
  // 新增：用 LayoutManager 的行数和最后一行 endIndex 推算排版内容的总 UTF-16 长度，
  // 与 pending text 的 UTF-16 长度比较——如果不匹配，说明 LayoutManager 排版的还是旧内容。
  // Issue #879 最新复核评论修复（问题2）：使用 latestCoreSnapshot 而非 currentSnapshot——
  // 验证 pending 身份时需要与 Core 最新编辑事实比较。
  private validatePendingIdentity(): boolean {
    const snap = this.latestCoreSnapshot ?? this.currentSnapshot
    if (snap === null) {
      return false
    }
    // 校验 revision/generation/compositionGeneration
    if (snap.revision !== this.pendingRevision) {
      return false
    }
    if (snap.generation !== this.pendingGeneration) {
      return false
    }
    if (snap.compositionGeneration !== this.pendingCompositionGeneration) {
      return false
    }
    // 校验 style（fontSize/lineSpacing）
    if (snap.fontSize !== this.pendingFontSize) {
      return false
    }
    if (snap.lineSpacingMultiplier !== this.pendingLineSpacing) {
      return false
    }
    // 校验 width（contentWidth 可能还在更新，用 renderResult 的行布局宽度校验）
    // contentWidth 来自 Text onAreaChange，layout observer 回调时可能已更新。
    // 如果 contentWidth 与 pending 不匹配，可能是宽度还没更新——保留 pending 等下一次回调。
    if (this.contentWidth !== this.pendingContentWidth && this.pendingContentWidth > 0) {
      return false
    }
    // Issue #879 本轮复核评论修复（问题4 增强）：用 LayoutManager 校验排版内容确实对应 pending revision。
    // 用最后一行的 endIndex 推算排版内容的总 UTF-16 长度，与 pending text 的 UTF-16 长度比较。
    // 如果不匹配，说明 LayoutManager 排版的还是旧内容——保留 pending 等下一次回调。
    const layout = this.textController.getLayoutManager()
    if (layout !== null && layout !== undefined) {
      const lineCount = layout.getLineCount()
      if (lineCount > 0) {
        const lastLineMetrics = layout.getLineMetrics(lineCount - 1)
        const layoutTextLength = lastLineMetrics.endIndex
        const pendingTextLength = snap.text.length
        if (layoutTextLength !== pendingTextLength) {
          // LayoutManager 排版的内容长度与 pending 不匹配——排版可能还是旧内容
          return false
        }
      }
    }
    return true
  }

  // Issue #879 复核评论修复（问题4）：更新主 Text 的静态裁切 path 命令。
  // 静态模式（非动画）不做裁切——全尺寸矩形 path。
  // 只有动画中且有 motion owner 时才计算遮罩 path。
  // 初始挂载阶段（contentWidth/contentHeight 为 0）也不裁切——用足够大的矩形。
  // 不再用空字符串——空字符串在内容尺寸为零时会构建宽高为 0 的矩形。
  // Issue #879 复核评论修复（问题1）：使用 displayedVisualFrame——
  // 静态裁切只跟随当前可见帧，不跟随候选帧。
  private updateStaticClipCommands(): void {
    if (this.displayedVisualFrame === null || !this.displayedVisualFrame.isAnimating ||
        !EditorMotionLayer.needsStaticMask(this.displayedVisualFrame)) {
      // 非动画或无 motion owner：全尺寸矩形（不裁切）
      if (this.contentWidth > 0 && this.contentHeight > 0) {
        this.staticClipCommands =
          `M0 0 L${this.contentWidth} 0 L${this.contentWidth} ${this.contentHeight} L0 ${this.contentHeight} Z`
      } else {
        this.staticClipCommands = 'M0 0 L99999 0 L99999 99999 L0 99999 Z'
      }
    } else {
      // 动画且有 motion owner：按 staticVisibleRanges 裁切
      // Issue #879 复核评论修复（问题6）：用 committedLineLayouts（后台排版结果）而非
      // renderResult.lines（refreshRender 后可能为空），确保静态遮罩有正确的行几何。
      this.staticClipCommands = EditorMotionLayer.buildStaticMaskPath(
        EditorMotionLayer.computeStaticMaskRects(
          this.displayedVisualFrame,
          this.committedLineLayouts,
          this.contentWidth,
          this.contentHeight
        )
      )
    }
  }

  // Issue #879 本轮复核评论修复（问题6）：统一停止动画入口。
  // 先增 generation 使旧 animator 的 onFrame/onFinish 回调失效，再 cancel/释放旧 animator。
  // 被静态重建、无动画更新、会话切换、设置变化与页面卸载调用。
  // 关键：必须先 ++motionGeneration 再 cancel——cancel 会额外触发一次终点 onFrame，
  // 先 cancel 再 ++generation 会让旧动画终值写到新画面。
  // Issue #879 复核评论修复（问题4）：停止动画时清理不再需要的源——
  // 如果不再有运动窗口，标记旧源可释放（保留当前 displayed 版本的源）。
  private stopActiveAnimation(): void {
    this.motionGeneration++
    if (this.motionAnimator !== null) {
      this.motionAnimator.cancel()
      this.motionAnimator = null
    }
    this.isMotionAnimating = false
    // Issue #879 复核评论修复（问题4）：停止动画后清理不再需要的源。
    this.releaseSourcesAfterMotion()
    // Issue #879 最新复核评论修复（问题1）：停止动画时清理所有窗口级 ComponentObserver。
    // stopActiveAnimation 只被静态重建、会话切换、设置变化、页面卸载这些
    // 「整帧作废」的路径调用（调用方同时丢掉 candidateToken/candidateVisualFrame），
    // 所以这里可以彻底清理；旧 animator 的 onFinish 不走这条路。
    this.cleanupAllWindowsObservers()
  }

  // Issue #879 复核评论修复（问题4）：统一所有权交接——动画结束后释放不再需要的源与 observer。
  //
  // Issue #879 复核评论6075662695问题1：旧 animator 的 onFinish 也会调本方法，
  // 而候选可能正在 staging（窗口已挂载、还没启动动画）。因此这里必须
  // **按 owner 释放**，不能全局清空：
  // - 源用 collectLiveSourceRoots()（displayed ∪ candidate ∪ 主文本版本）做存活根，
  //   候选/旧 displayed 窗口的源不会被误收；
  // - observer 只针对「不属于 displayed/candidate 帧」的窗口解绑，
  //   新候选尚未完成的观察者、计时与待提交身份都保持不变。
  // 会话彻底退出时由 stopActiveAnimation/会话切换路径清空全部 observer。
  private releaseSourcesAfterMotion(): void {
    this.motionSourcePool.releaseUnusedSources(this.collectLiveSourceRoots())
    const liveKeys: Set<string> = new Set<string>()
    const frames: (EditorVisualFrame | null)[] = [this.displayedVisualFrame, this.candidateVisualFrame]
    for (const frame of frames) {
      if (frame === null || !frame.isAnimating) {
        continue
      }
      for (const w of frame.motionGlyphWindows) {
        liveKeys.add(w.renderNodeKey)
      }
    }
    const keysToDelete: string[] = []
    this.windowLayoutObservers.forEach((_observer: inspector.ComponentObserver, key: string) => {
      if (!liveKeys.has(key)) {
        keysToDelete.push(key)
      }
    })
    for (const key of keysToDelete) {
      this.detachWindowLayoutObserver(key)
    }
  }

  // Issue #879 第三轮复核修复：逐帧动画驱动——用 AnimatorResult.onFrame 实现真正的逐帧回调。
  // animateTo + @Watch 只能收到 0/1 终值，无法逐帧驱动 sampleFrame。
  // AnimatorResult.onFrame(progress) 是 ArkUI 渲染层每帧回调，progress 从 0 到 1 连续变化。
  // motionGeneration 计数器防止旧 animator 的完成事件结束新动画。
  // Issue #879 本轮复核评论修复（问题3）：
  // - 必须先 ++motionGeneration 再 cancel 旧 animator：华为 AnimatorResult.cancel() 会额外触发
  //   一次终点 onFrame。当前代码是先 cancel 再 ++motionGeneration，有机会把旧动画终值写到新画面。
  //   改为先 ++motionGeneration 再 cancel，旧 animator 的 onFrame 回调因 generation 检查失效。
  // - onFrame 改为调 advance(progress)（sampleFrame 不回写 currentFrame，advance 回写）。
  // Issue #879 本轮复核评论修复（问题6）：
  // - 用 getUIContext().createAnimator(options) 替代 Animator.create(options)。
  // - AnimatorOptions 从 @kit.ArkUI 导入。
  private startMotionAnimation(durationMs: number): void {
    if (durationMs <= 0) {
      // 无动画时长，直接跳到终态
      this.displayedVisualFrame = this.motionCoordinator.finishMotion()
      this.candidateVisualFrame = null
      this.isMotionAnimating = false
      return
    }
    // Issue #879 复核评论6075662695问题2：本方法只由 tryCommitCandidate 调用，
    // 而它已经在提交前确认过 allMotionWindowsReady()——这里不再暂存任何等待状态
    // （等待状态属于候选 token，不属于这个全局方法）。
    if (!this.allMotionWindowsReady()) {
      // 防御：窗口在提交与启动之间被解绑。此时不启动动画，保持已提交的起始帧，
      // 等属于本候选的窗口回调再次触发提交（候选仍在 staging 时不会走到这里）。
      return
    }
    // Issue #879 本轮复核评论修复（问题3）：先 ++motionGeneration 再 cancel 旧 animator。
    // 华为 AnimatorResult.cancel() 会额外触发一次终点 onFrame，
    // 先 cancel 再 ++generation 会让旧动画终值写到新画面。
    // 先 ++generation 再 cancel，旧 animator 的 onFrame 回调因 generation 检查失效。
    this.motionGeneration++
    const currentGen = this.motionGeneration
    // 取消旧 animator（cancel 触发的 onFrame 会因 currentGen !== this.motionGeneration 而失效）
    if (this.motionAnimator !== null) {
      this.motionAnimator.cancel()
      this.motionAnimator = null
    }
    this.isMotionAnimating = true

    // Issue #879 本轮复核评论修复（问题6）：用 getUIContext().createAnimator 替代 Animator.create。
    // Animator.create 从 API18 起废弃，@kit.ArkUI 导出 AnimatorOptions 和 AnimatorResult。
    const animatorOptions: AnimatorOptions = {
      duration: durationMs,
      easing: 'easeOut',
      delay: 0,
      fill: 'none',
      direction: 'normal',
      iterations: 1,
      begin: 0,
      end: 1
    }
    this.motionAnimator = this.getUIContext().createAnimator(animatorOptions)
    // Issue #879 本轮复核评论修复（问题3）：onFrame 改为调 advance(progress)。
    // sampleFrame 不回写 currentFrame，advance 调 sampleFrame 后回写 currentFrame 并返回。
    // 确保下一次输入 acceptUpdate 用的 startFrame = currentFrame 是当前进度的真实状态。
    this.motionAnimator.onFrame = (progress: number): void => {
      if (currentGen !== this.motionGeneration) {
        return // 旧 animator 回调，忽略
      }
      this.displayedVisualFrame = this.motionCoordinator.advance(progress)
      // Issue #879 本轮复核评论修复（问题3）：每帧更新主 Text 的静态裁切 path。
      this.updateStaticClipCommands()
    }
    this.motionAnimator.onFinish = (): void => {
      if (currentGen !== this.motionGeneration) {
        return
      }
      this.isMotionAnimating = false
      this.displayedVisualFrame = this.motionCoordinator.finishMotion()
      // Issue #879 复核评论修复（问题3）：动画结束后更新裁切 path 为全尺寸矩形。
      this.updateStaticClipCommands()
      // Issue #879 复核评论修复（问题4）：动画结束后所有运动窗口消失，
      // 标记旧源可释放并真正释放（保留当前 displayed 版本的源，因为可能下一笔编辑还需要）。
      this.releaseSourcesAfterMotion()
      this.motionAnimator = null
    }
    this.motionAnimator.play()
  }

  // Issue #768 评论5837229456：把"生成 StyledString"和"写进 TextController"分开。
  // API15 之前，在 aboutToAppear 里通过 TextController.setStyledString() 设置内容无法显示，
  // 因为 Text 组件还未创建。用 textMounted 守卫确保只在 Text.onAppear 之后才下发 StyledString。
  // Issue #879 本轮复核评论修复（问题2）：newStyledString 已在 onPresentationUpdateChanged 中构造。
  // applyStyledText 只负责把 StyledString 写入主 Text 的 TextController，不再重新构造 newStyledString。
  private applyStyledText(): void {
    if (!this.textMounted) {
      return
    }
    // Issue #879 复核评论修复（问题3）：使用 displayedStyledString 而非 newStyledString——
    // newStyledString 是 pending 版本（新内容），displayedStyledString 是当前正在显示的版本。
    // 在 onPendingLayoutReady 中 displayedStyledString 已更新为新版本，此时写入主 Text 是正确的。
    if (this.displayedSnapshot === null) {
      this.textController.setStyledString(new MutableStyledString(''))
      return
    }
    // displayedStyledString 已在 onPendingLayoutReady/aboutToAppear 中设置
    if (this.displayedStyledString !== null) {
      this.textController.setStyledString(this.displayedStyledString)
    }
  }

  // 基础渲染：用 render() 转 offset，不含几何（lines/rects 为空）。
  // aboutToAppear/aboutToUpdate 调用；onAreaChange 触发后改用 refreshRenderLayout 补几何。
  // Issue #629 评论11 第2项：refreshRender 不再用 render() 的空 lines 覆盖 lineResolver。
  // render() 不计算几何（lines: []），用它覆盖会把 resolver 清空。lineResolver 只在
  // refreshRenderLayout（contentWidth > 0，有真实几何）时更新（带身份）。
  // Issue #768 评论5837229456：不再在 refreshRender 里直接调 setStyledString，
  // 改为通过 applyStyledText()（受 textMounted 守卫）下发 StyledString。
  // Issue #879 复核评论修复（问题1）：refreshRender 现在只在后台排版完成后（onPendingLayoutReady）
  // 或初始加载（aboutToAppear）时调用——不再在 onPresentationUpdateChanged 中立即调用。
  // 主 Text 在编辑期间保持显示旧内容，直到后台排版完成后才更新。
  private refreshRender(): void {
    // Issue #879 复核评论修复（问题3）：使用 displayedSnapshot 来生成基础渲染结果——
    // refreshRender 在 onPendingLayoutReady 中调用（此时 displayedSnapshot 已更新为新版本），
    // 或在初始加载（aboutToAppear）时调用（此时 displayedSnapshot 已初始化）。
    if (this.displayedSnapshot) {
      // 仍然用 EditorRenderBackend.render() 转 offset（不含几何）
      const result = EditorRenderBackend.render(this.displayedSnapshot)
      this.renderResult = result
      this.selectionRects = result.selectionRects
      this.caretRect = result.caretRect
      this.compositionUnderlineRects = result.compositionUnderlineRects
    } else {
      this.renderResult = null
      this.selectionRects = []
      this.caretRect = null
      this.compositionUnderlineRects = []
      // snapshot 为 null 时清空 resolver + 取消等待（无文本无布局）。
      this.lineResolver.cancelWait()
      this.lineResolver.updateLayout(null)
    }
    this.applyStyledText()
  }

  // 完整静态渲染：用 renderLayout() 计算行布局/选区/光标/composition 下划线几何。
  // 在 onAreaChange 中调用（此时 contentWidth/contentHeight 已更新，getUIContext 可用）。
  // Issue #629 评论11 第2项：aboutToUpdate 也调此方法（contentWidth 已 > 0 时），
  // 保证 snapshot/revision/composition 变化时立刻用已有宽度计算真实 lines 并更新 lineResolver，
  // 不等 onAreaChange 才更新。lineResolver 带身份（revision + displayText + cursorUtf16），
  // executeLineNavigation 出队时验证身份匹配才用 lines 算目标。
  // Issue #768 任务9：优先使用系统 LayoutManager 做唯一布局真源。
  // - API14+：用 getRectsForRange 获取选区/光标/composition 矩形，完全不需要 measureText
  // - API12-13：用 LineMetrics 获取行边界（折行/行高），用 measureText 算行内 x 坐标
  // - Fallback：LayoutManager 不可用时走旧的自计算路径
  private refreshRenderLayout(): void {
    // Issue #879 复核评论修复（问题3）：主 Text observer 只用 displayed 版本生成渲染几何——
    // 使用 displayedSnapshot 而非 currentSnapshot（可能已被 pending 更新覆盖）。
    if (this.displayedSnapshot && this.contentWidth > 0) {
      const snap = this.displayedSnapshot

      // 优先使用系统 LayoutManager
      const layout = this.textController.getLayoutManager()
      if (layout !== null && layout !== undefined) {
        const systemLineCount = layout.getLineCount()
        if (systemLineCount > 0) {
          this.refreshRenderLayoutFromSystem(layout, systemLineCount, snap)
          return
        }
      }

      // Fallback: 旧的自计算路径（LayoutManager 不可用或无行）
      // Issue #768 评论5835171266 第5项：记录 fallback 原因和递增计数。
      this.fallbackCount++
      if (layout === null || layout === undefined) {
        this.lastLayoutError = 'LayoutManager unavailable'
      } else {
        this.lastLayoutError = 'LayoutManager returned 0 lines'
      }
      this.refreshRenderLayoutFallback(snap)
    }
  }

  // Issue #768 任务10：从系统 LayoutManager 获取真实布局。
  // 逻辑已下沉到 EditorRenderBackend.renderLayoutFromSystem() 和
  // LineNavigationResolver.buildLayoutStateFromSystem()。
  private refreshRenderLayoutFromSystem(
    layout: LayoutManager,
    systemLineCount: number,
    snap: EditorLayoutSnapshot,
  ): void {
    try {
      const cursorUtf16 = TextOffsetMapper.utf8ToUtf16(snap.text, snap.cursorByteOffset)
      const visualCaret = this.selectionController?.getVisualCaret(cursorUtf16)
        ?? { utf16Offset: cursorUtf16, affinity: CaretAffinity.Downstream }
      const isApi14Plus = PlatformApiResolver.getInstance().isApiAtLeast(14)

      // measureTextFn：API12-13 fallback 用，API14+ 不需要但保持接口一致
      // Issue #776 评论5848626733 第6项：measureUtils.measureText 返回 px，
      // fallback 几何统一到 vp，这里包一层 px2vp。
      const measureUtils = this.getUIContext().getMeasureUtils()
      const measureTextFn = (s: string): number => {
        return this.getUIContext().px2vp(measureUtils.measureText({ textContent: s, fontSize: snap.fontSize }))
      }

      // Issue #776 评论5848626733 第6项：系统 LayoutManager 返回 px，renderLayoutFromSystem
      // 内部用 pxToVp 把 LineMetrics/getRectsForRange 的 px 转成 vp。
      const pxToVp = (value: number): number => this.getUIContext().px2vp(value)
      // Issue #776 评论5849108212 问题2：vpToPx 用于行导航 getNearestOffsetFromSystem
      // 把 vp 坐标转回 px 传给系统 API。
      const vpToPx = (value: number): number => this.getUIContext().vp2px(value)

      // 1. 调用 EditorRenderBackend.renderLayoutFromSystem 获取完整渲染结果
      const result = EditorRenderBackend.renderLayoutFromSystem(
        layout,
        systemLineCount,
        snap,
        this.contentWidth,
        visualCaret.affinity,
        isApi14Plus,
        measureTextFn,
        pxToVp,
      )

      // 2. 更新 renderResult 和几何状态
      this.renderResult = result
      this.selectionRects = result.selectionRects
      this.caretRect = result.caretRect
      this.compositionUnderlineRects = result.compositionUnderlineRects

      // 3. 更新行级导航解析器（带身份）
      // Issue #768 评论5836390597 第2项：传入 measureTextFn 让 buildLayoutStateFromSystem
      // 能用 buildLineCaretStops + metrics.left 偏移构建 caretStops，
      // API12~13 的 Up/Down 行导航不再跳到行首。
      const layoutState = LineNavigationResolver.buildLayoutStateFromSystem(
        layout,
        systemLineCount,
        snap,
        this.contentWidth,
        measureTextFn,
        pxToVp,
        vpToPx,
      )
      this.lineResolver.updateLayout(layoutState)
      // Issue #768：标记布局来源为 system 并推送诊断快照（布局更新时）。
      this.lastLayoutSource = 'system'
      // Issue #768 评论5835171266 第5项：系统路径成功时清除错误。
      this.lastLayoutError = null
      // Issue #768 评论5837824666：更新布局身份字段，标记当前布局对应的 snapshot 身份。
      this.layoutRevision = snap.revision
      this.layoutGeneration = snap.generation
      this.layoutCompositionGeneration = snap.compositionGeneration
      this.visualCaretAffinity = visualCaret.affinity === CaretAffinity.Upstream ? 'upstream' : 'downstream'
      // Issue #768 评论5837824666：缓存系统布局行数和当前行度量，供 pushDiagnosticsSnapshot 使用。
      this.lastSystemLayoutLineCount = systemLineCount
      this.lastCurrentLineMetrics = this.extractCurrentLineMetrics(layout, systemLineCount, cursorUtf16, visualCaret.affinity)
      this.pushDiagnosticsSnapshot()
    } catch (e) {
      // Issue #768 评论5835171266 第5项：记录系统布局错误，回退到 fallback。
      this.lastLayoutError = `system layout failed: ${String(e)}`
      this.fallbackCount++
      this.refreshRenderLayoutFallback(snap)
    }
  }

  // Issue #768 任务9：Fallback 路径 — LayoutManager 不可用时走旧的自计算路径。
  private refreshRenderLayoutFallback(snap: EditorLayoutSnapshot): void {
    // Issue #776 评论5848626733 第6项：measureUtils.measureText 返回 px，
    // fallback 几何统一到 vp，这里包一层 px2vp。
    const measureUtils = this.getUIContext().getMeasureUtils()
    const measureTextFn = (s: string): number => {
      return this.getUIContext().px2vp(measureUtils.measureText({ textContent: s, fontSize: snap.fontSize }))
    }
    const cursorUtf16 = TextOffsetMapper.utf8ToUtf16(
      snap.text,
      snap.cursorByteOffset,
    )
    const visualCaret = this.selectionController?.getVisualCaret(cursorUtf16)
      ?? { utf16Offset: cursorUtf16, affinity: CaretAffinity.Downstream }
    const result = EditorRenderBackend.renderLayout(
      snap,
      this.contentWidth,
      measureTextFn,
      visualCaret.affinity,
    )
    this.renderResult = result
    this.selectionRects = result.selectionRects
    this.caretRect = result.caretRect
    this.compositionUnderlineRects = result.compositionUnderlineRects
    const navigationLines: NavigationLine[] = result.lines
    const layoutState: LineLayoutState = {
      revision: snap.revision,
      generation: snap.generation,
      compositionGeneration: snap.compositionGeneration,
      compositionSessionId: snap.compositionSessionId,
      displayText: snap.text,
      contentWidth: this.contentWidth,
      fontSize: snap.fontSize,
      lines: navigationLines,
      // Fallback 路径无系统 LayoutManager，行导航回退到 caretStops 方案。
      systemLayoutManager: null,
      // Issue #776 评论5849108212 问题2：fallback 路径也提供 pxToVp/vpToPx，
      // 虽然 fallback 行导航不走系统 API，但保持接口一致。
      pxToVp: (value: number): number => this.getUIContext().px2vp(value),
      vpToPx: (value: number): number => this.getUIContext().vp2px(value),
    }
    this.lineResolver.updateLayout(layoutState)
    // Issue #768：标记布局来源为 fallback 并推送诊断快照（布局更新时）。
    this.lastLayoutSource = 'fallback'
    // Issue #768 评论5837824666：更新布局身份字段。
    this.layoutRevision = snap.revision
    this.layoutGeneration = snap.generation
    this.layoutCompositionGeneration = snap.compositionGeneration
    this.visualCaretAffinity = visualCaret.affinity === CaretAffinity.Upstream ? 'upstream' : 'downstream'
    // Issue #768 评论5837824666：缓存系统布局行数（fallback 路径无 LayoutManager，行数为 0）
    // 和当前行度量（从 renderResult.lines 获取，但 baseline/left/topHeight 不可用）。
    this.lastSystemLayoutLineCount = 0
    this.lastCurrentLineMetrics = this.extractCurrentLineMetricsFromRenderResult(result, cursorUtf16)
    this.pushDiagnosticsSnapshot()
  }

  // Issue #768 评论5838624438：从真实 LayoutManager.LineMetrics 提取当前行度量。
  // 直接从 layout.getLineMetrics(index) 记录 left/topHeight/height/baseline/startIndex/endIndex，
  // 不再拿 renderResult.lines 拼一个残缺的 currentLineMetrics。
  // Issue #768 评论5838624438：soft-wrap 边界按 affinity 选择上行或下行，
  // 和 caret 规则保持一致：Upstream → 上一视觉行，Downstream → 下一视觉行。
  private extractCurrentLineMetrics(
    layout: LayoutManager,
    systemLineCount: number,
    cursorUtf16: number,
    affinity: CaretAffinity,
  ): CurrentLineMetricsSnapshot | null {
    try {
      for (let i = 0; i < systemLineCount; i++) {
        const metrics = layout.getLineMetrics(i)
        if (cursorUtf16 >= metrics.startIndex && cursorUtf16 <= metrics.endIndex) {
          // soft-wrap 边界：同一个 UTF-16 offset 同时是上一行 endIndex 和下一行 startIndex。
          // Upstream → 上一视觉行（当前匹配行），Downstream → 下一视觉行（i+1）。
          if (cursorUtf16 === metrics.endIndex
            && affinity === CaretAffinity.Downstream
            && i + 1 < systemLineCount) {
            const nextMetrics = layout.getLineMetrics(i + 1)
            return {
              startIndex: nextMetrics.startIndex,
              endIndex: nextMetrics.endIndex,
              height: nextMetrics.height,
              baseline: nextMetrics.baseline,
              left: nextMetrics.left,
              topHeight: nextMetrics.topHeight,
            }
          }
          return {
            startIndex: metrics.startIndex,
            endIndex: metrics.endIndex,
            height: metrics.height,
            baseline: metrics.baseline,
            left: metrics.left,
            topHeight: metrics.topHeight,
          }
        }
      }
    } catch (e) {
      hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG,
        `extractCurrentLineMetrics: getLineMetrics failed: ${JSON.stringify(e)}`)
    }
    return null
  }

  // Issue #768 评论5837824666：Fallback 路径从 renderResult.lines 提取当前行度量。
  // LineLayout 不含 baseline/left/topHeight，这些字段填 0 表示不可用。
  private extractCurrentLineMetricsFromRenderResult(
    result: EditorRenderResult,
    cursorUtf16: number,
  ): CurrentLineMetricsSnapshot | null {
    const lines = result.lines
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i]
      if (cursorUtf16 >= line.startUtf16 && cursorUtf16 <= line.endUtf16) {
        return {
          startIndex: line.startUtf16,
          endIndex: line.endUtf16,
          height: line.height,
          baseline: 0,
          left: 0,
          topHeight: 0,
        }
      }
    }
    return null
  }

  // Issue #629 评论18：命中测试返回 VisualCaretPosition（含 CaretAffinity）。
  // soft-wrap 边界上半点击返回 Upstream（上一行尾），下半返回 Downstream（下一行首）。
  // Issue #768 任务10：优先使用系统 LayoutManager 的 getGlyphPositionAtCoordinate 做命中测试，
  // 消除自算折行与系统布局的错位。Fallback 到旧的自计算路径。
  // 命中测试逻辑已下沉到 EditorRenderBackend.hitTestByLayoutManager()。
  // Issue #879 最新复核评论修复（问题2）：使用 displayedSnapshot 而非 currentSnapshot——
  // LayoutManager 对应的是 displayedSnapshot（主 Text 当前显示的内容）。
  // 后台候选等待期间 currentSnapshot 可能已更新为新版本，但主 Text 还在显示旧内容。
  private hitTestContent(contentX: number, contentY: number): VisualCaretPosition {
    // Issue #879 最新复核评论修复（问题2）：优先使用 displayedSnapshot（对应主 Text 的 LayoutManager）
    const snap = this.displayedSnapshot ?? this.currentSnapshot
    if (!snap || !this.renderResult || this.contentWidth <= 0) {
      return { utf16Offset: 0, affinity: CaretAffinity.Downstream }
    }

    // 优先使用系统 LayoutManager
    const layout = this.textController.getLayoutManager()
    if (layout !== null && layout !== undefined) {
      try {
        // Issue #776 评论5848626733 第6项：getGlyphPositionAtCoordinate 接收 px，
        // contentX/contentY 是编辑器本地 vp 坐标，先转成 px。
        const pxX = this.getUIContext().vp2px(contentX)
        const pxY = this.getUIContext().vp2px(contentY)
        return EditorRenderBackend.hitTestByLayoutManager(layout, pxX, pxY)
      } catch (e) {
        hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG,
          `hitTestContent: getGlyphPositionAtCoordinate failed: ${JSON.stringify(e)}`)
      }
    }

    // Fallback: 旧的自计算命中测试——使用 displayedSnapshot
    // Issue #776 评论5848626733 第6项：measureUtils.measureText 返回 px，
    // fallback 几何统一到 vp，这里包一层 px2vp。
    const measureUtils = this.getUIContext().getMeasureUtils()
    const measureTextFn = (s: string): number => {
      return this.getUIContext().px2vp(measureUtils.measureText({ textContent: s, fontSize: snap.fontSize }))
    }
    return EditorRenderBackend.hitTestByPoint(
      snap,
      this.contentWidth,
      contentX,
      contentY,
      measureTextFn,
    )
  }

  // Issue #879 复核评论修复（问题2）：把可见修订（displayed）的 UTF-16 位置映射到 Core 最新修订。
  //
  // 命中测试用的是 displayedSnapshot + 主 Text 的 LayoutManager，得到的是「屏幕那一版」
  // 的位置；而 dispatcher 出队时按 Core 最新 snapshot 解释该位置。
  // 两阶段发布等待期间 displayed 落后 Core（候选还没提交），二者不是同一坐标系，
  // 因此必须用待发布编辑链把位置映射过去，不能把旧版 offset 直接当新版 offset 发出去。
  //
  // 返回 null 表示映射不可得（版本不同但链不可信），调用方不应把未经转换的旧 offset 发出去。
  //
  // Issue #879 复核评论修复（问题2）：目标 revision 用 latestCoreSnapshot（在
  // onPresentationUpdateChanged 中即刻跟随 Core），链必须满足：
  // - 同 session：链上每一笔都属于当前编辑会话；
  // - revision 连续：每笔 baseRevision → newRevision 连到下一笔，首尾对齐 displayed/latest；
  // - 文本重放一致：每笔 patch 重放结果等于该笔给出的布局正文（RevisionPositionMap.build 校验）。
  // 任一项不成立就返回 null——调用方宁可不发命令，也不把旧版 offset 冒充新版。
  private buildDisplayedToCorePositionMap(): RevisionPositionMap | null {
    const from = this.displayedSnapshot
    const to = this.latestCoreSnapshot
    if (!from || !to || from.revision === to.revision) {
      return null
    }
    if (this.pendingPresentationUpdates.length === 0) {
      return null
    }
    const sessionId = this.presentationUpdate?.sessionId ?? this.pendingPresentationUpdates[0].sessionId
    const steps: PositionMapStep[] = []
    let beforeText = from.text
    for (const update of this.pendingPresentationUpdates) {
      if (update.sessionId !== sessionId) {
        // 跨会话的更新不属于同一条链——不能用来做位置映射。
        return null
      }
      const editResult = update.editResult
      if (editResult === null) {
        // 非编辑更新（纯光标移动等）不改正文——不产生位置映射步。
        continue
      }
      const afterText = update.layoutSnapshot.text
      if (editResult.newRevision !== update.layoutSnapshot.revision) {
        // 编辑事实（newRevision）与布局事实（layoutSnapshot.revision）对不上——
        // 无法证明这一笔就是这份正文的成因，放弃映射。
        return null
      }
      if (editResult.newRevision === editResult.baseRevision) {
        // 不推进 revision 的一笔（IME composition begin/update/cancel）：preedit 不在
        // snapshot.text 里，正文与坐标系都没变——不产生映射步。
        // 若这类步却改了正文，说明链的语义不成立，宁可放弃映射。
        if (afterText !== beforeText) {
          return null
        }
        continue
      }
      steps.push({
        baseRevision: editResult.baseRevision,
        nextRevision: editResult.newRevision,
        beforeText: beforeText,
        afterText: afterText,
        patches: EditorMotionCoordinator.collectCarryPatches(editResult)
      })
      beforeText = afterText
    }
    if (steps.length === 0 || beforeText !== to.text) {
      return null
    }
    return RevisionPositionMap.build(steps, from.revision, to.revision)
  }

  // Issue #879 复核评论修复（问题2）：把可见坐标转成 Core 最新修订坐标后再发 dispatcher。
  // 映射不可得时返回 null——调用方必须检查 null，不应把未经转换的旧 UTF-16 offset
  // 冒充新版直接提交给 dispatcher（中间删字/插字/跨行后的偏移变化无法靠长度夹紧解决）。
  private toCoreCaretPosition(position: VisualCaretPosition): VisualCaretPosition | null {
    const from = this.displayedSnapshot
    const to = this.latestCoreSnapshot
    if (!from || !to) {
      // 缺少必要的 snapshot——无法安全映射
      return null
    }
    if (from.revision === to.revision) {
      // 同一 revision——无需映射，直接夹紧返回
      const offset = Math.max(0, Math.min(position.utf16Offset, to.text.length))
      return { utf16Offset: offset, affinity: position.affinity }
    }
    // 不同 revision——必须有有效映射才能转换位置
    const map = this.buildDisplayedToCorePositionMap()
    if (map === null) {
      // 版本不同但映射不可得——不能把旧 offset 冒充新 offset
      return null
    }
    const offset = Math.max(0, Math.min(map.mapOffset(position.utf16Offset), to.text.length))
    return { utf16Offset: offset, affinity: position.affinity }
  }

  // 当前光标 UTF-16 offset（从 snapshot.cursorByteOffset 转）。
  // Issue #879 最新复核评论修复（问题2）：使用 displayedSnapshot 而非 currentSnapshot——
  // 光标位置应与当前显示的内容对应，而非可能已更新但尚未显示的新版本。
  private currentCursorUtf16(): number {
    const snap = this.displayedSnapshot ?? this.currentSnapshot
    if (!snap) {
      return 0
    }
    return TextOffsetMapper.utf8ToUtf16(snap.text, snap.cursorByteOffset)
  }

  // 当前选区 [anchorUtf16, headUtf16]（从 snapshot 的 byte offset 转）。
  // Issue #879 最新复核评论修复（问题2）：使用 displayedSnapshot 而非 currentSnapshot——
  // 选区位置应与当前显示的内容对应。
  private currentSelectionUtf16(): [number, number] {
    const snap = this.displayedSnapshot ?? this.currentSnapshot
    if (!snap) {
      return [0, 0]
    }
    const text = snap.text
    const anchor = TextOffsetMapper.utf8ToUtf16(text, snap.selectionAnchor)
    const head = TextOffsetMapper.utf8ToUtf16(text, snap.selectionHead)
    return [anchor, head]
  }

  // 是否有非空选区（anchor != head）。
  // Issue #879 最新复核评论修复（问题2）：使用 displayedSnapshot 而非 currentSnapshot。
  private hasSelection(): boolean {
    const snap = this.displayedSnapshot ?? this.currentSnapshot
    if (!snap) {
      return false
    }
    return snap.selectionAnchor !== snap.selectionHead
  }

  // Issue #767 评论 5829138873 第 1 节：focusAndAttachIme 在 requestFocus 成功后直接 attach IME。
  // 旧实现只调 requestFocus，attach 仅在 .onFocus() 回调里做——但组件已获焦时 requestFocus
  // 不会再次触发 onFocus，第一次 attach 失败后用户再点击正文无法重试。
  // 现在 requestFocus 成功后直接调 harmonyImeConnection.attach(dispatcher)，确保每次点击都有重试入口。
  // .onFocus() 保留 attach 作为键盘导航/无障碍等非点击获焦路径的兜底。
  // HarmonyImeConnection.attach() 已是串行幂等（唯一 transitionTask 链），
  // TouchDown 和 onFocus 都发起 attach 只会排进同一条串行链，不会并发 controller.attach()。
  private focusAndAttachIme(): void {
    if (!this.harmonyImeConnection || !this.dispatcher) {
      hilog.warn(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'focusAndAttachIme: no imeConnection or dispatcher')
      return
    }
    hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'focusAndAttachIme: requesting focus')
    try {
      this.getUIContext().getFocusController().requestFocus(INPUT_HOST_ID)
    } catch (e) {
      hilog.error(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, `focusAndAttachIme: requestFocus failed: ${JSON.stringify(e)}`)
      return
    }
    // requestFocus 成功后直接 attach IME——每次点击都有重试入口，
    // 不依赖 onFocus 是否再次触发（组件已获焦时 requestFocus 不会重复触发 onFocus）。
    const imeConnection = this.harmonyImeConnection
    const dispatcher = this.dispatcher
    imeConnection.attach(dispatcher).then(() => {
      hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'focusAndAttachIme: IME attach success')
      // Issue #768：IME attach 成功后推送诊断快照。
      this.pushDiagnosticsSnapshot()
    }).catch((e: Object) => {
      hilog.error(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, `focusAndAttachIme: IME attach failure: ${JSON.stringify(e)}`)
    })
  }

  // ── 键盘语义命令分发 ──
  // 把 SemanticKeyCommand 转成语义命令经 EditorSemanticDispatcher 派发。
  // 原始 KeyEvent 不进 Rust；这里只派发语义命令。
  // Issue #629 评论6 Part A：所有输入经 dispatcher 统一排队，出队时才读 snapshot。
  // grapheme-based 导航（backspace/delete/arrowLeft/arrowRight）用 Core grapheme boundary（Part E），
  // 不再用 UTF-16 ±1 猜字符边界。
  // Issue #629 评论11 第2项：arrowUp/arrowDown/home/end 改用语义命令 lineUp/lineDown/lineHome/lineEnd，
  // 不在入队前读旧布局计算目标。出队时由 LineNavigationResolver 基于最新行布局计算。
  // Issue #879 最新复核评论修复（问题2）：使用 displayedSnapshot 而非 currentSnapshot——
  // 键盘命令应基于当前显示的内容派发。
  private dispatchKeyCommand(cmd: SemanticKeyCommand): void {
    const snap = this.displayedSnapshot ?? this.currentSnapshot
    if (!this.dispatcher || !snap) {
      return
    }
    const dispatcher: EditorSemanticDispatcher = this.dispatcher
    switch (cmd.kind) {
      case 'character':
        // insertText：由 EditorInputAdapter 判断当前是否有选区（有选区替换，空选区插入）
        dispatcher.dispatchFireAndForget({ kind: 'insertText', text: cmd.text })
        break
      case 'enter':
        dispatcher.dispatchFireAndForget({ kind: 'insertText', text: '\n' })
        break
      case 'tab':
        dispatcher.dispatchFireAndForget({ kind: 'insertText', text: '\t' })
        break
      case 'backspace':
        // grapheme-based backspace：用 Core previous_grapheme_boundary 找前一个 grapheme 边界
        dispatcher.dispatchFireAndForget({ kind: 'graphemeBackspace' })
        break
      case 'delete':
        // grapheme-based delete：用 Core next_grapheme_boundary 找后一个 grapheme 边界
        dispatcher.dispatchFireAndForget({ kind: 'graphemeDelete' })
        break
      case 'arrowLeft':
        // grapheme-based left：用 Core previous_grapheme_boundary
        dispatcher.dispatchFireAndForget({ kind: 'graphemeLeft', extend: cmd.extend })
        break
      case 'arrowRight':
        // grapheme-based right：用 Core next_grapheme_boundary
        dispatcher.dispatchFireAndForget({ kind: 'graphemeRight', extend: cmd.extend })
        break
      case 'arrowUp': {
        // Issue #629 评论11 第2项：行级导航改用语义命令 lineUp，
        // 不在入队前读旧布局计算目标。出队时由 LineNavigationResolver 基于最新行布局计算。
        dispatcher.dispatchFireAndForget({ kind: 'lineUp', extend: cmd.extend })
        break
      }
      case 'arrowDown': {
        // Issue #629 评论11 第2项：行级导航改用语义命令 lineDown。
        dispatcher.dispatchFireAndForget({ kind: 'lineDown', extend: cmd.extend })
        break
      }
      case 'home': {
        // Issue #629 评论11 第2项：行级导航改用语义命令 lineHome。
        dispatcher.dispatchFireAndForget({ kind: 'lineHome', extend: cmd.extend })
        break
      }
      case 'end': {
        // Issue #629 评论11 第2项：行级导航改用语义命令 lineEnd。
        dispatcher.dispatchFireAndForget({ kind: 'lineEnd', extend: cmd.extend })
        break
      }
      case 'escape':
      case 'ignore':
        // 不处理。
        break
      default:
        break
    }
  }

  // ── 鼠标事件处理 ──
  // Issue #629 R11 评论5329310563 第2项：鼠标/触摸 selection 命令统一走 dispatch 的 Promise，
  // 成功后再从 dispatcher 查询最终 display selection 通知 IME。
  // 不在 Core 完成前通知 IME（避免拿原 hitPos 猜未来状态），也不漏通知（result.success 时必通知）。
  // dispatcher 失败（result.success=false）不通知 IME。
  private dispatchSelectionAndSyncIme(cmd: SemanticEditCommand): void {
    const dispatcher = this.dispatcher
    if (!dispatcher) {
      return
    }
    dispatcher.dispatch(cmd).then((result) => {
      if (result.success) {
        this.harmonyImeConnection?.syncSelectionFromEditor().then(() => {})
      }
    }, (_e: Object) => {
      // 命令失败不阻塞后续；错误已由 Dispatcher/Coordinator 处理
    })
  }

  // 点击命中、按下拖动选区、抬起结束选择经 dispatcher 派发；滚轮由 Scroll 组件自动消费。
  // Issue #629 评论6 Part A：所有输入经 dispatcher 统一排队，不再直接调 SelectionController。
  // Issue #629 R11 评论5329310563 第2项：mouse down/drag 走 dispatchSelectionAndSyncIme，
  // 成功后查询最终 selection 再通知 IME（鼠标 selection 之前完全没通知 IME）。
  // Issue #879 最新复核评论修复（问题2）：使用 displayedSnapshot 而非 currentSnapshot——
  // 命中测试和选区操作应基于当前显示的内容。
  private handleMouseEvent(raw: MouseEvent): void {
    const snap = this.displayedSnapshot ?? this.currentSnapshot
    if (!snap || !this.renderResult || !this.dispatcher) {
      return
    }
    const ev: PlatformPointerEvent = this.pointerService.normalizeMouseEvent(raw)
    // Issue #778 评论5853666955 第1项：onMouse 绑定在内层 Stack 上，
    // ArkUI MouseEvent.x/y 已是相对 Stack 左上角的局部坐标，滚动后也由 ArkUI 自动补偿。
    // 不再调用 viewportToContent()（会把滚动量算第二次），也不再减 padding（padding 在外层 Column）。
    const contentX = ev.x
    const contentY = ev.y
    const inContentArea =
      contentX >= 0 &&
      contentY >= 0 &&
      contentX <= this.contentWidth &&
      contentY <= this.contentHeight
    // Issue #629 评论18：hitTestContent 返回 VisualCaretPosition。
    const hitPos: VisualCaretPosition = inContentArea
      ? this.hitTestContent(contentX, contentY)
      : { utf16Offset: 0, affinity: CaretAffinity.Downstream }
    switch (ev.action) {
      case 'down':
        if (ev.button === 0) {
          // Issue #767 评论 5828146609 第 1 节：鼠标点击也先确保 focus/IME。
          this.focusAndAttachIme()
          // Issue #776 评论5848626733 第8项：padding 区域只获焦，不改变 selection。
          if (!inContentArea) {
            break
          }
          // 左键按下：记 anchor position，开始 potential drag。
          this.dragAnchor = hitPos
          this.dragAnchorCore = this.toCoreCaretPosition(hitPos)
          this.isDragging = true
          // Issue #879 复核评论修复（问题2）：映射不可得时不发送 dispatcher 命令——
          // 不应把未经转换的旧 UTF-16 offset 冒充新版直接提交。
          if (this.dragAnchorCore !== null) {
            this.dispatchSelectionAndSyncIme({ kind: 'setSelection', position: this.dragAnchorCore })
          }
        }
        break
      case 'move':
        if (this.isDragging && this.dragAnchorCore !== null && inContentArea) {
          // 拖动选区：anchor 不变，head 跟随当前位置。传完整 VisualCaretPosition。
          // Issue #879 复核评论修复（问题2）：anchor 已是 Core 坐标，head 现场映射；
          // 映射不可得时不发送 dispatcher 命令。
          const headCore = this.toCoreCaretPosition(hitPos)
          if (headCore !== null) {
            this.dispatchSelectionAndSyncIme({
              kind: 'dragSelect',
              anchor: this.dragAnchorCore,
              head: headCore
            })
          }
        }
        break
      case 'up':
        if (this.isDragging) {
          this.isDragging = false
          this.dragAnchor = null
          this.dragAnchorCore = null
        }
        break
      case 'wheel':
        // 滚轮由 Scroll 组件自动消费（改 viewport），onScroll 回调把新 viewport 状态回调出去。
        // 此处不额外处理。
        break
      default:
        break
    }
  }

  // ── 触摸事件处理 ──
  // 复用现有 hit test，补真实 drag selection（TouchDown 记 anchor, TouchMove drag, TouchUp 结束）。
  // 笔事件（sourceTool === Pen）由 StylusInputService 识别后按触控选择/书写区交互处理。
  // Issue #629 评论6 Part A+D：所有输入经 dispatcher 统一排队；删除重复 onSelectionTap prop（双 owner）。
  // Issue #629 R11 评论5329310563 第2项：TouchUp/TouchMove drag 走 dispatchSelectionAndSyncIme，
  // 成功后查询最终 selection 再通知 IME，不再 fire-and-forget 后立即用原 hitPos 通知。
  // Issue #879 最新复核评论修复（问题2）：使用 displayedSnapshot 而非 currentSnapshot——
  // 命中测试和选区操作应基于当前显示的内容。
  private handleTouchEvent(event: TouchEvent): void {
    const snap = this.displayedSnapshot ?? this.currentSnapshot
    if (!snap || !this.renderResult || !this.dispatcher) {
      return
    }
    // Issue #743 评论5792119958：空触点边界检查必须先于 touches[0] 解引用。
    // ArkUI TouchEvent.touches 在 Cancel 等场景可能为空；StylusInputService.normalizeStylusEvent
    // 虽已处理空 touches，但编辑器在调用 service 前先解引用 touches[0] 会让 service 的 null 防护走不到。
    // 华为 ArkUI 文档明确：使用 touches 前需先检查是否为空。
    // Cancel 场景顺手清掉 isDragging / dragAnchor，避免留下上一轮拖选状态；其余空触点直接 return。
    if (event.touches.length === 0) {
      if (event.type === TouchType.Cancel) {
        this.isDragging = false
        this.dragAnchor = null
      }
      return
    }
    const touch: TouchObject = event.touches[0]
    // Issue #767 评论 5828146609 第 1 节：首次触摸编辑区时先确保 focus/IME，
    // 这样空白处点击也能获得焦点和键盘。
    if (event.type === TouchType.Down) {
      hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, `touch down: x=${touch.x}, y=${touch.y}`)
      this.focusAndAttachIme()
    }
    // Issue #778 评论5853666955 第1项：onTouch 绑定在内层 Stack 上，
    // ArkUI TouchObject.x/y 已是相对 Stack 左上角的局部坐标，滚动后也由 ArkUI 自动补偿。
    // 不再调用 viewportToContent()（会把滚动量算第二次），也不再减 padding（padding 在外层 Column）。
    const contentX = touch.x
    const contentY = touch.y
    const inContentArea =
      contentX >= 0 &&
      contentY >= 0 &&
      contentX <= this.contentWidth &&
      contentY <= this.contentHeight
    // Issue #629 评论18：hitTestContent 返回 VisualCaretPosition。
    const hitPos: VisualCaretPosition = inContentArea
      ? this.hitTestContent(contentX, contentY)
      : { utf16Offset: 0, affinity: CaretAffinity.Downstream }

    // 笔事件识别：统一由 StylusInputService.isStylusEvent 按 ArkUI event.sourceTool 判断。
    // 由 StylusInputService 归一化后按触控选择/书写区交互处理，原始 stylus event 不进 Core。
    if (this.stylusService.isStylusEvent(event)) {
      this.stylusService.normalizeStylusEvent(event)
      // 笔事件按触控选择/书写区交互处理（与触控同一套 hit test + selection）。
    }

    switch (event.type) {
      case TouchType.Down:
        // Issue #776 评论5848626733 第8项：padding 区域不命中文字，不开始 drag。
        if (!inContentArea) {
          break
        }
        // 按下：记 anchor，开始 potential drag。
        this.dragAnchor = hitPos
        this.dragAnchorCore = this.toCoreCaretPosition(hitPos)
        this.isDragging = true
        break
      case TouchType.Move:
        // 拖动选区：anchor 不变，head 跟随当前位置。传完整 VisualCaretPosition。
        // Issue #879 复核评论修复（问题2）：比较用可见坐标，发命令用 Core 坐标；
        // 映射不可得时不发送 dispatcher 命令。
        if (this.isDragging && this.dragAnchor !== null && this.dragAnchorCore !== null
          && inContentArea && this.dragAnchor.utf16Offset !== hitPos.utf16Offset) {
          const headCore = this.toCoreCaretPosition(hitPos)
          if (headCore !== null) {
            this.dispatchSelectionAndSyncIme({
              kind: 'dragSelect',
              anchor: this.dragAnchorCore,
              head: headCore
            })
          }
        }
        break
      case TouchType.Up:
        if (this.isDragging) {
          if (inContentArea && this.dragAnchor !== null
            && this.dragAnchor.utf16Offset === hitPos.utf16Offset) {
            // 单击：经 dispatcher 发 selection，成功后查询最终 selection 再通知 IME。
            // Issue #879 复核评论修复（问题2）：映射不可得时不发送 dispatcher 命令。
            const corePos = this.toCoreCaretPosition(hitPos)
            if (corePos !== null) {
              this.dispatchSelectionAndSyncIme({ kind: 'setSelection', position: corePos })
            }
          }
          this.isDragging = false
          this.dragAnchor = null
          this.dragAnchorCore = null
        }
        break
      default:
        break
    }
  }

  // viewport 状态变化时回调给 WritingScreen。
  // Issue #629 评论6 Part C：用真实 viewportWidth/viewportHeight（外层 Scroll 可视区域）
  // 和 contentWidth/contentHeight（内层正文实际渲染尺寸）构造状态。
  // 旧实现把 viewportHeight = textHeight、contentHeight = textHeight（两者永远相同），
  // maxScrollTop = contentHeight - viewportHeight 永远 0；新实现分开记录，maxScrollTop 可正确 > 0。
  private emitViewportChange(xOffset: number, yOffset: number): void {
    if (!this.onViewportChange) {
      return
    }
    // Issue #776 评论5849108212 问题5：报告 Scroll 内容总高度（含 padding）。
    const scrollContentHeight = this.contentHeight + EDITOR_TOP_PADDING_VP + EDITOR_BOTTOM_PADDING_VP
    const state: EditorViewportState = new EditorViewportState(
      yOffset,
      xOffset,
      this.viewportWidth,
      this.viewportHeight,
      this.contentWidth,
      scrollContentHeight
    )
    this.onViewportChange(state)
  }

  // Issue #629 评论6 Part C：Stack 高度：至少可用区域高度，随正文增长。
  // 旧实现固定 .height('100%')，长正文真实内容高度被压成视口高度，maxScrollTop 永远 0。
  // 新实现：contentHeight > availableHeight 时用 contentHeight（长正文可滚动）；
  // 否则用 availableHeight（短正文至少占满视口）；都是 0 时用 '100%' 兜底避免高度 0。
  // Issue #778 任务7：padding 从 Stack 移到外层 Column，Stack 高度不含 padding。
  // availableHeight = viewportHeight - 上下 padding（Column 的 padding 占用了一部分 viewport 高度）。
  private stackHeight(): number | string {
    const availableHeight = Math.max(0, this.viewportHeight - EDITOR_TOP_PADDING_VP - EDITOR_BOTTOM_PADDING_VP)
    if (this.contentHeight > 0 && this.contentHeight > availableHeight) {
      return this.contentHeight
    }
    if (availableHeight > 0) {
      return availableHeight
    }
    return '100%'
  }

  build() {
    // Scroll 包裹内容，实现真实可滚动 viewport。
    // 正文+选区+composition+caret 在同一滚动坐标系里（Stack align(TopStart)）。
    // 滚轮由 Scroll 自动消费；onScroll 回调把 viewport 状态回调给 WritingScreen。
    Scroll(this.scroller) {
      // Issue #778 任务7：padding 从 Stack 移到外层 Column，Stack 只做编辑坐标系。
      // Column 负责留白（阅读边距），Stack 内所有坐标基于自身左上角（不含 padding）。
      Column() {
        // Issue #767 评论 5828146609 第 1 节：Stack 作为唯一输入宿主（INPUT_HOST_ID）。
        // 焦点和输入事件挂在整个全尺寸 Stack 上，不再挂在正文 Text 上。
        // 正文 Text 只负责绘制文字，不再承担"可点击输入框"职责。
        // 这样空白处点击也能命中宿主节点，触发 focus + IME attach。
        Stack() {
        if (this.renderResult && (this.displayedSnapshot ?? this.currentSnapshot)) {
          // 1. selection background（最底层）
          ForEach(this.selectionRects, (rect: SelectionRect, index: number) => {
            Rect()
              .position({ x: rect.x, y: rect.y })
              .width(rect.width)
              .height(rect.height)
              .fill(this.selectionBackgroundColor)
          }, (rect: SelectionRect, index: number) => index.toString())

          // 2. text — 只负责绘制文字，不再承担焦点/输入事件职责。
          // Issue #768 任务9：Text 接 TextController，内容通过 StyledString 下发（含字号/行高/首行缩进）。
          // wordBreak(BREAK_ALL) 与 fallback 路径的逐 code point 折行一致。
          // 字号、行高、首行缩进通过 StyledString 的 TextStyle/LineHeightStyle/ParagraphStyle 下发，
          // 不再在 Text 属性上设置 fontSize。
          // Issue #879 本轮复核评论修复（问题3）：始终保留一个主 Text 节点，不再用 if/else 切换。
          // 用外层 Stack 的 clipShape 绑定到 @State staticClipCommands 控制裁切：
          // 非动画时为全尺寸矩形（不裁切）；动画时按 staticVisibleRanges 裁切。
          // Text 节点本身不重建——只有 clipShape 的 commands 在帧间变化。
          Stack() {
            Text(undefined, { controller: this.textController })
              .id(EDITOR_TEXT_ID)
              .width('100%')
              .textAlign(TextAlign.Start)
              .wordBreak(WordBreak.BREAK_ALL)
              .onAppear(() => {
                // Issue #768 评论5837229456：API15 之前，在 aboutToAppear 里通过
                // TextController.setStyledString() 设置内容无法显示，因为组件还未创建。
                // 在 Text.onAppear 后才标记挂载并下发 StyledString。
                // Issue #768 评论5837824666：在 onAppear 中注册布局观察者，
                // 确保后续 setStyledString 后通过 layout 回调（而非同步读取）获取 LayoutManager。
                this.textMounted = true
                this.attachTextLayoutObserver()
                this.applyStyledText()
                // 不再同步调 refreshRenderLayout() — 等 ComponentObserver 'layout' 回调触发。
              })
              .onDisAppear(() => {
                // Issue #768 评论5837824666：注销布局观察者。
                this.detachTextLayoutObserver()
                this.textMounted = false
              })
              .onAreaChange((_oldArea: Area, newArea: Area): void => {
                // Issue #629 评论6 Part C：Text onAreaChange 更新的是内层正文实际渲染尺寸
                // （contentWidth/contentHeight），不是外层 Scroll 可视区域尺寸。
                const oldContentWidth = this.contentWidth
                this.contentWidth = Number(newArea.width)
                this.contentHeight = Number(newArea.height)
                // Issue #629 R9 评论5326213302 第4项：内容宽度变化时失效旧 visual caret hint。
                // 布局宽度变化不改 Core revision，但 soft-wrap 可能重新发生，旧 affinity 可能不再有效。
                // 不给 SujianEditor 再加第二份 affinity 状态；invalidate 后 refreshRenderLayout 从 owner 重取。
                if (this.contentWidth !== oldContentWidth && this.selectionController) {
                  this.selectionController.invalidateVisualCaret()
                }
                // 内容尺寸已知后重新计算几何（选区/光标/composition 下划线）。
                this.refreshRenderLayout()
                // Issue #629 评论7 第4项：内容尺寸更新后通知 controller 重试消费 pending。
                // register 时尺寸可能为 0，pending 保留；onAreaChange 拿到真实尺寸后才消费。
                if (this.viewportController) {
                  this.viewportController.notifyDimensionsChanged()
                }
              })
          }
          .width('100%')
          .clipShape(new PathShape({
            commands: this.staticClipCommands
          }))

          // 3. motion layer — Issue #879 复核评论修复（问题1）：运动层同时渲染可见窗口和候选窗口。
          // displayedVisualFrame 的窗口 visible=true（正常显示），
          // candidateVisualFrame 的窗口 visible=false（opacity 0，参与布局但不可见）。
          // 候选窗口 layoutReady 后原子提交时 visible 变为 true。
          // 吐字：新字源完整排版（newStyledString），clipShape 限制在 [x0, front] 裁切窗口。
          // 吞字：旧字源完整排版（oldStyledString），clipShape 限制在 [x0, front] 裁切窗口。
          if ((this.displayedVisualFrame !== null && this.displayedVisualFrame.isAnimating &&
               EditorMotionLayer.needsMotionLayer(this.displayedVisualFrame)) ||
              (this.candidateVisualFrame !== null && this.candidateVisualFrame.isAnimating &&
               EditorMotionLayer.needsMotionLayer(this.candidateVisualFrame))) {
            MotionTextOverlayBuilder({
              motionWindowSources: this.buildAllMotionWindowSources(),
              textWidth: this.contentWidth
            })
          }

          // 4. composition underline（在 text 上面）
          ForEach(this.compositionUnderlineRects, (rect: CompositionUnderlineRect, index: number) => {
            Rect()
              .position({ x: rect.x, y: rect.y })
              .width(rect.width)
              .height(rect.height)
              .fill(this.compositionUnderlineColor)
          }, (rect: CompositionUnderlineRect, index: number) => index.toString())

          // 5. caret（最上层）— Issue #879：单一视觉光标。
          // 动画中：光标位置由 displayedVisualFrame 决定（画在裁切前沿 front）。
          // 非动画：光标画在静态位置（caretRect）。
          // Issue #879 复核评论修复（问题1）：使用 displayedVisualFrame 而非 candidateVisualFrame——
          // 光标只跟随当前可见帧，不跟随候选帧。
          if (this.displayedVisualFrame !== null && this.displayedVisualFrame.isAnimating) {
            // 动画中：光标位置由运动协调器的视觉帧决定
            Rect()
              .position({ x: this.displayedVisualFrame.caretX, y: this.displayedVisualFrame.caretY })
              .width(2)
              .height(this.displayedVisualFrame.caretHeight)
              .fill(this.caretColor)
          } else if (this.caretRect) {
            // 非动画：光标在静态位置
            Rect()
              .position({ x: this.caretRect.x, y: this.caretRect.y })
              .width(this.caretRect.width)
              .height(this.caretRect.height)
              .fill(this.caretColor)
          }

          // Issue #879 复核评论修复（问题1）：隐藏排版源——后台排版新 StyledString，不影响可见主 Text。
          // EditorPendingLayoutHost 在隐藏 Text 中排版新 StyledString，布局完成后回调行布局结果，
          // 让 SujianEditor 在同一帧发布新静态可见区 + 运动源窗口 + 光标。
          // 通过 @State pendingLayoutRequest + @Prop + @Watch 模式触发排版。
          // Issue #879 复核评论修复（问题1）：StyledString 不走 @Prop 深拷贝——
          // 通过 resolveLayoutRequest(token) 回调从 styledStringByToken 获取原始引用。
          EditorPendingLayoutHost({
            layoutRequest: this.pendingLayoutRequest,
            resolveLayoutRequest: (token: number): StyledString | null => {
              return this.styledStringByToken.get(token) ?? null
            },
            onLayoutReady: (token: number, lineLayouts: LineLayout[], firstCaretRect?: CaretRect): void => {
              this.onPendingLayoutReady(token, lineLayouts, firstCaretRect)
            }
          })
        } else {
          Text('')
        }
      }
      .id(INPUT_HOST_ID)
      .focusable(true)
      .onFocus((): void => {
        // Issue #767 评论 5829138873 第 1 节：onFocus 作为键盘导航/无障碍等非点击获焦路径的兜底 attach。
        // 点击获焦路径由 focusAndAttachIme() 直接调 attach（确保第一次失败后再次点击能重试）。
        // HarmonyImeConnection.attach() 已是串行幂等（唯一 transitionTask 链），
        // focusAndAttachIme 和 onFocus 都发起 attach 只会排进同一条串行链，不会并发 controller.attach()。
        hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'onFocus: input host focus acquired')
        // Issue #768：记录焦点状态并推送诊断快照。
        this.isFocused = true
        this.pushDiagnosticsSnapshot()
        if (this.harmonyImeConnection && this.dispatcher) {
          const imeConnection = this.harmonyImeConnection
          const dispatcher = this.dispatcher
          imeConnection.attach(dispatcher).then(() => {
            hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'onFocus: IME attach success')
            // Issue #768：IME attach 成功后推送诊断快照。
            this.pushDiagnosticsSnapshot()
          }).catch((e: Object) => {
            hilog.error(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, `onFocus: IME attach failure: ${JSON.stringify(e)}`)
          })
        }
      })
      .onBlur((): void => {
        // Issue #767 评论 5828448634 第 1 节：编辑区失焦 → detach IME。
        hilog.info(EDITOR_LOG_DOMAIN, EDITOR_LOG_TAG, 'onBlur: input host blur')
        // Issue #768：记录失焦状态并推送诊断快照。
        this.isFocused = false
        this.pushDiagnosticsSnapshot()
        if (this.harmonyImeConnection) {
          this.harmonyImeConnection.detach().then(() => {
            // Issue #768：IME detach 完成后推送诊断快照。
            this.pushDiagnosticsSnapshot()
          })
        }
      })
      .onKeyEvent((event: KeyEvent): boolean => {
        // 物理键盘事件 → KeyboardInputService 归一化 → 语义命令 → dispatcher.dispatch。
        // 原始 KeyEvent 不进 Rust。所有输入经 dispatcher 统一排队（Part A）。
        const platformEv: PlatformKeyEvent = this.keyboardService.normalize(event)
        const cmd: SemanticKeyCommand = this.keyboardService.toSemanticCommand(platformEv)
        this.dispatchKeyCommand(cmd)
        // 消费已识别的命令（非 ignore），防止事件冒泡到系统默认处理。
        return cmd.kind !== 'ignore'
      })
      .onMouse((event: MouseEvent): boolean => {
        // 鼠标/触控板事件 → PointerInputService 归一化 → 点击/drag/滚轮。
        this.handleMouseEvent(event)
        return false
      })
      .onTouch((event: TouchEvent): void => {
        // 触摸/笔事件 → hit test + drag selection。
        this.handleTouchEvent(event)
      })
      .align(Alignment.TopStart)
      .width('100%')
      // Issue #629 评论6 Part C：Stack 高度：至少可用区域高度，随正文增长。
      // 旧实现固定 .height('100%')，长正文真实内容高度被压成视口高度，maxScrollTop 永远 0。
      // 新实现：contentHeight > availableHeight 时用 contentHeight（长正文可滚动）；
      // 否则用 availableHeight（短正文至少占满视口）；都是 0 时用 '100%' 兜底避免高度 0。
      .height(this.stackHeight())
      }
      .width('100%')
      // Issue #778 任务7：正文阅读边距从 Stack 移到外层 Column。
      // Column 负责留白，Stack 内坐标不含 padding。
      .padding({
        left: EDITOR_HORIZONTAL_PADDING_VP,
        right: EDITOR_HORIZONTAL_PADDING_VP,
        top: EDITOR_TOP_PADDING_VP,
        bottom: EDITOR_BOTTOM_PADDING_VP,
      })
    }
    .scrollable(ScrollDirection.Vertical)
    .scrollBar(BarState.Auto)
    .width('100%')
    .height('100%')
    .onAreaChange((_oldArea: Area, newArea: Area): void => {
      // Issue #629 评论6 Part C：Scroll onAreaChange 更新的是外层可视区域尺寸
      // （viewportWidth/viewportHeight），不是内层正文实际渲染尺寸。
      this.viewportWidth = Number(newArea.width)
      this.viewportHeight = Number(newArea.height)
      // Issue #629 评论7 第4项：viewport 尺寸更新后通知 controller 重试消费 pending。
      // register 时尺寸可能为 0，pending 保留；onAreaChange 拿到真实尺寸后才消费。
      if (this.viewportController) {
        this.viewportController.notifyDimensionsChanged()
      }
      // Issue #768：视口尺寸变化时推送诊断快照。
      this.pushDiagnosticsSnapshot()
    })
    .onScroll((xOffset: number, yOffset: number): void => {
      // 滚动时把 viewport 状态回调给 WritingScreen（供接续状态持续写入）。
      this.emitViewportChange(xOffset, yOffset)
      // Issue #768：滚动变化时推送诊断快照（含 currentScrollX/Y）。
      this.pushDiagnosticsSnapshot()
    })
  }
}
