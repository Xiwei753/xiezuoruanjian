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
import { EditorMotionCoordinator } from '../motion/EditorMotionCoordinator'
import { EditorMotionLayer, ClipRect, MotionTextOverlayBuilder, MotionTextOverlayParams, MotionWindowSource } from '../motion/EditorMotionLayer'
// Issue #879 最新复核评论修复（问题1）：EditorEditResult 用于收集事务链。
import { EditorEditResult } from '../../../corebridge/dto/EditorDtos'
// Issue #879 复核评论修复（问题1/3/4）：前后台排版隔离 + 稳定运动字形源池。
import { EditorPendingLayoutHost, PendingLayoutRequest } from './EditorPendingLayoutHost'
import { EditorMotionSourcePool } from '../motion/EditorMotionSourcePool'
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
  @State currentSnapshot: EditorLayoutSnapshot | null = null
  // Issue #879：运动协调器——接收编辑更新，生成运动规划，输出视觉帧。
  private motionCoordinator: EditorMotionCoordinator = new EditorMotionCoordinator()
  // Issue #879：当前视觉帧——描述某一时刻的完整视觉状态（光标位置、静态/运动层字形范围）。
  @State private visualFrame: EditorVisualFrame | null = null
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
  // onPresentationUpdateChanged 暂存 pendingPresentationUpdate，等 layout 回调后才调 updateVisualFrame。
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
  // Issue #879 复核评论修复（问题4）：每个运动窗口独立的 TextController——
  // ArkUI 不允许同一 TextController 绑定多个 Text，所以每个窗口需要独立 Controller。
  // key 是 clusterId（基于 sourceKind + sourceRevision + utf16Start + utf16End 构造的稳定身份），
  // 与 ForEach key 一致，确保复用时 Controller 保持稳定，窗口增减/顺序变化不会错配。
  private motionWindowControllers: Map<string, TextController> = new Map()
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
  private dragAnchor: VisualCaretPosition | null = null
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
    }
    this.refreshRender()
    // Issue #879 复核评论修复（问题4）：初始化静态裁切 path 为全尺寸矩形（不裁切）。
    this.updateStaticClipCommands()
    // Issue #879 复核评论修复：不立即调 updateVisualFrame()——Text layout 尚未完成。
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
    // Issue #879 复核评论修复（问题3）：清空运动源池。
    this.motionSourcePool.clear()
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
  // 从 this.currentSnapshot、几何状态、IME 状态、滚动状态等读取数据构造完整快照。
  // 不写正文内容，只写长度、offset、几何和状态。在各生命周期点调用。
  private pushDiagnosticsSnapshot(): void {
    const store = EditorRuntimeSnapshotStore.getInstance()
    const snap = this.currentSnapshot

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
      this.visualFrame = null
      this.pendingPresentationUpdates = []
      this.previousLayout = []
      // Issue #879 复核评论修复（问题3）：会话切换时清空 styledStringByToken、consumedTokens、pendingLayoutRequest。
      this.styledStringByToken.clear()
      this.consumedTokens.clear()
      this.pendingLayoutRequest = null
      // Issue #879 复核评论修复（问题3）：清空运动源池。
      this.motionSourcePool.clear()
      // Issue #879 复核评论修复（问题4）：会话切换时清空所有窗口 Controller。
      this.motionWindowControllers.clear()
      // Issue #879 复核评论修复（问题4）：会话失效时更新裁切 path 为全尺寸矩形。
      this.updateStaticClipCommands()
      // Issue #768：会话失效时推送诊断快照（hasActiveSession=false）。
      this.pushDiagnosticsSnapshot()
      return
    }
    // Issue #879 复核评论修复（问题3）：不立即切换 currentSnapshot——
    // 保持 currentSnapshot 指向 displayed 版本（旧内容），新内容暂存到 pending 字段。
    // currentSnapshot 在 onPendingLayoutReady 中才更新为新版本。

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
      const request: PendingLayoutRequest = {
        token: this.pendingLayoutToken,
        contentWidth: this.contentWidth,
        fontSize: fontSize,
        fullTextLength: fullTextLength,
        styleKey: `${fontSize}-${lineSpacing}-${autoIndentEnabled}-${autoIndentWidth}`,
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
  // - 用后台排版结果（lineLayouts）作为新布局，调 updateVisualFrameWithLayout
  // - 不用等主 Text 的 layout observer 回调——后台排版结果已经是正确的行布局
  //
  // Issue #879 最新复核评论修复（问题1）：消费整个事务链——
  // 从 pendingPresentationUpdates 数组中取出所有事务，收集所有 editResult，
  // 调用 motionCoordinator.acceptUpdateWithChain 合并事务链。
  private onPendingLayoutReady(token: number, lineLayouts: LineLayout[]): void {
    // Issue #879 复核评论修复（问题3）：要求 pendingPresentationUpdates 非空才消费。
    if (token !== this.pendingLayoutToken || this.pendingPresentationUpdates.length === 0) {
      return
    }
    // Issue #879 复核评论修复（问题3）：检查已消费的 token——防止同一 token 重复消费。
    if (this.consumedTokens.has(token)) {
      return
    }
    this.consumedTokens.add(token)
    // Issue #879 复核评论修复（问题3）：后台排版结果获批后，先将 displayed 指向新版本，
    // 然后才把新 StyledString 写入主 Text，然后才调 updateVisualFrameWithLayout。
    // 1. 取最新版本作为 displayed 版本
    const latestUpdate = this.pendingPresentationUpdates[this.pendingPresentationUpdates.length - 1]
    this.displayedSnapshot = latestUpdate.layoutSnapshot
    this.displayedLineLayouts = lineLayouts
    this.displayedStyledString = this.newStyledString
    // 2. 更新 currentSnapshot——业务状态切换到新版本
    this.currentSnapshot = latestUpdate.layoutSnapshot
    // 3. 保存后台排版的行布局——作为 Coordinator 和 computeStaticMaskRects 的唯一相同输入
    this.committedLineLayouts = lineLayouts
    // 4. 把新 StyledString 写入主 Text（触发主 Text 的 layout）
    this.refreshRender()
    // 5. 收集所有 editResult 形成事务链，调 updateVisualFrameWithLayout
    this.updateVisualFrameWithLayout(lineLayouts)
    // 6. 更新 displayedPresentation——当前正在显示的版本（在清空 pending 之前保存）
    this.displayedPresentation = latestUpdate
    // 7. 清空 pendingPresentationUpdates——防止重复消费
    this.pendingPresentationUpdates = []
    // Issue #879 复核评论修复（问题1）：清理已消费的 token→StyledString 映射条目
    this.styledStringByToken.delete(token)
  }

  // Issue #879 复核评论修复（问题1）：用后台排版结果更新视觉帧。
  // 类似 updateVisualFrame()，但接收外部传入的 lineLayouts（来自 EditorPendingLayoutHost），
  // 而不是从 renderResult.lines 读取。
  // Issue #879 最新复核评论修复（问题1）：收集所有 pendingPresentationUpdates 的 editResult，
  // 调用 acceptUpdateWithChain 合并事务链，而非只传单个 editResult。
  private updateVisualFrameWithLayout(newLineLayouts: LineLayout[]): void {
    if (this.currentSnapshot === null || this.pendingPresentationUpdates.length === 0) {
      return
    }
    const oldSnapshot = this.previousSnapshot ?? this.currentSnapshot
    // 收集所有 pendingPresentationUpdates 的 editResult 形成事务链
    const editResults: EditorEditResult[] = []
    for (const update of this.pendingPresentationUpdates) {
      if (update.editResult !== null) {
        editResults.push(update.editResult)
      }
    }
    // Issue #879 最新复核评论修复（问题1）：调用 acceptUpdateWithChain 合并事务链。
    // 如果事务链只有一个 editResult，acceptUpdateWithChain 内部走原来的 acceptUpdate 逻辑。
    this.visualFrame = this.motionCoordinator.acceptUpdateWithChain(
      oldSnapshot,
      this.previousLayout,
      this.currentSnapshot,
      newLineLayouts,
      editResults,
      this.contentWidth,
      this.motionPolicy
    )
    // 用 motionSourcePool 准备运动字形源
    this.prepareMotionSources()
    // 更新静态裁切 path（只在有 motion owner 时才裁切）
    this.updateStaticClipCommands()
    // 启动动画或停止旧动画
    if (this.motionCoordinator.isAnimating()) {
      const durationMs = this.motionCoordinator.getCurrentPlan().durationMs
      this.startMotionAnimation(durationMs)
    } else {
      this.stopActiveAnimation()
    }
  }

  // Issue #879 复核评论修复（问题3）：用 EditorMotionSourcePool 替代旧的 controller 数组。
  // 源池用 sourceRevision + stableRunId 保存 StyledString/TextController，
  // ForEach key 基于稳定 run 身份（runId），而非一次性自增事务编号。
  // Issue #879 复核评论修复（问题4）：池所有权绑定到当前显示帧实际使用的源——
  // 不再被任何窗口引用且不属于 displayed/pending 版本的源立即标记释放并清理。
  private prepareMotionSources(): void {
    if (this.visualFrame === null || !this.visualFrame.isAnimating) {
      return
    }
    // Issue #879 复核评论修复（问题4）：释放不再被任何窗口引用且不属于 displayed/pending 版本的源。
    // 获取当前 visualFrame 的 motionGlyphWindows 中所有 runId
    const activeRunIds: Set<string> = new Set()
    const windows = this.visualFrame.motionGlyphWindows
    for (let i = 0; i < windows.length; i++) {
      activeRunIds.add(windows[i].runId)
    }
    const displayedRevision = this.displayedSnapshot?.revision ?? 0
    const pendingRevision = this.pendingRevision
    this.motionSourcePool.releaseUnusedSources(activeRunIds, displayedRevision, pendingRevision)

    // 清理不再需要的旧窗口 Controller——只保留当前窗口需要的
    // Issue #879 复核评论修复（问题4）：controller key 改为 clusterId（稳定身份），
    // 不再使用 `${runId}-${index}`（数组序号），避免窗口增减/顺序变化错配 controller。
    const neededKeys: Set<string> = new Set()
    for (let i = 0; i < windows.length; i++) {
      neededKeys.add(windows[i].clusterId)
    }
    const keysToDelete: string[] = []
    this.motionWindowControllers.forEach((_: TextController, key: string) => {
      if (!neededKeys.has(key)) {
        keysToDelete.push(key)
      }
    })
    for (const key of keysToDelete) {
      this.motionWindowControllers.delete(key)
    }

    // 为运动窗口获取或创建源
    const oldRevision = this.previousSnapshot?.revision ?? 0
    const newRevision = this.currentSnapshot?.revision ?? 0

    if (this.oldStyledString !== null) {
      this.motionSourcePool.getOrCreateSource('old', oldRevision, this.oldStyledString)
    }
    if (this.newStyledString !== null) {
      this.motionSourcePool.getOrCreateSource('new', newRevision, this.newStyledString)
    }
  }

  // 预匹配运动窗口与源——在 build() 之前调用，避免在 @Builder 中做匹配（ArkUI 限制）。
  // ForEach key 基于 window.clusterId（稳定身份），而非 `${runId}-${index}`（数组序号）。
  // Issue #879 复核评论修复（问题4）：clusterId 基于 sourceKind + sourceRevision + utf16Start + utf16End
  // 构造的稳定身份标识，窗口增减/顺序变化不会把另一个簇绑定到旧 controller。
  private buildMotionWindowSources(): MotionWindowSource[] {
    if (this.visualFrame === null || !this.visualFrame.isAnimating) {
      return []
    }
    const clipRects = EditorMotionLayer.buildAllClipRects(this.visualFrame)
    const result: MotionWindowSource[] = []
    const windows = this.visualFrame.motionGlyphWindows
    for (let i = 0; i < windows.length; i++) {
      const source = this.motionSourcePool.getSource(windows[i].runId)
      if (source !== undefined) {
        // Issue #879 复核评论修复（问题4）：每个窗口独立的 TextController——
        // key 使用 clusterId（稳定身份），与 ForEach key 一致，确保复用时 Controller 保持稳定。
        const controllerKey = windows[i].clusterId
        let controller = this.motionWindowControllers.get(controllerKey)
        if (controller === undefined) {
          controller = new TextController()
          this.motionWindowControllers.set(controllerKey, controller)
        }
        result.push({
          window: windows[i],
          source: source,
          clipRect: clipRects[i],
          textController: controller,
          mounted: false
        })
      }
    }
    return result
  }

  // Issue #879 本轮复核评论修复（问题4）：校验 pending RenderPlan 身份。
  // layout observer 回调中核实 sessionId/revision/generation/composition/style/width 与 pending 一致。
  // 需保留 per-revision 的旧布局，不要在两次更新尚未消费之间用 previousLayout 一个变量覆盖旧版来源。
  // Issue #879 本轮复核评论修复（问题4 增强）：用 LayoutManager 校验排版内容确实对应 pending revision。
  // 旧实现只比 currentSnapshot 和 pendingRevision/generation/style/width——它们当然相等（同一份数据）。
  // 新增：用 LayoutManager 的行数和最后一行 endIndex 推算排版内容的总 UTF-16 长度，
  // 与 pending text 的 UTF-16 长度比较——如果不匹配，说明 LayoutManager 排版的还是旧内容。
  private validatePendingIdentity(): boolean {
    if (this.currentSnapshot === null) {
      return false
    }
    // 校验 revision/generation/compositionGeneration
    if (this.currentSnapshot.revision !== this.pendingRevision) {
      return false
    }
    if (this.currentSnapshot.generation !== this.pendingGeneration) {
      return false
    }
    if (this.currentSnapshot.compositionGeneration !== this.pendingCompositionGeneration) {
      return false
    }
    // 校验 style（fontSize/lineSpacing）
    if (this.currentSnapshot.fontSize !== this.pendingFontSize) {
      return false
    }
    if (this.currentSnapshot.lineSpacingMultiplier !== this.pendingLineSpacing) {
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
        const pendingTextLength = this.currentSnapshot.text.length
        if (layoutTextLength !== pendingTextLength) {
          // LayoutManager 排版的内容长度与 pending 不匹配——排版可能还是旧内容
          return false
        }
      }
    }
    return true
  }

  // Issue #879：通过运动协调器接收编辑更新，计算当前视觉帧。
  // 当 presentationUpdate 包含 editResult 时，运动协调器会规划文字运动（吐字/吞字/光标移动等）；
  // 当 editResult 为 null（初始加载/布局重建）时，直接显示静态画面。
  // Issue #879 复核评论修复：
  // - 使用 acceptUpdate 新签名（oldSnapshot + oldLayout + newSnapshot + newLayout + editResult + contentWidth + policy）
  // - 如果 isAnimating()，启动逐帧驱动
  // Issue #879 第三轮复核修复：使用 this.previousLayout（在 onPresentationUpdateChanged 中缓存），
  // 不再从参数接收 oldLineLayouts——旧布局在 refreshRender 之前已缓存。
  private updateVisualFrame(): void {
    if (this.currentSnapshot === null || this.pendingPresentationUpdates.length === 0) {
      this.visualFrame = null
      // Issue #879 本轮复核评论修复（问题3）：visualFrame 清空时更新裁切 path 为全尺寸矩形。
      this.updateStaticClipCommands()
      return
    }
    // 从 renderResult 获取新行布局信息，供运动协调器计算字形位置。
    const newLineLayouts = this.renderResult?.lines ?? []
    // oldSnapshot = previousSnapshot（编辑前的 snapshot）
    const oldSnapshot = this.previousSnapshot ?? this.currentSnapshot
    // 收集所有 pendingPresentationUpdates 的 editResult 形成事务链
    const editResults: EditorEditResult[] = []
    for (const update of this.pendingPresentationUpdates) {
      if (update.editResult !== null) {
        editResults.push(update.editResult)
      }
    }
    this.visualFrame = this.motionCoordinator.acceptUpdateWithChain(
      oldSnapshot,
      this.previousLayout,  // 使用缓存的旧布局
      this.currentSnapshot,
      newLineLayouts,
      editResults,
      this.contentWidth,
      this.motionPolicy
    )
    // Issue #879 复核评论修复（问题3）：用 motionSourcePool 准备运动字形源。
    this.prepareMotionSources()
    // Issue #879 复核评论修复（问题4）：更新主 Text 的静态裁切 path 命令。
    this.updateStaticClipCommands()
    // Issue #879 复核评论修复：如果运动协调器正在动画，启动逐帧驱动。
    if (this.motionCoordinator.isAnimating()) {
      const durationMs = this.motionCoordinator.getCurrentPlan().durationMs
      this.startMotionAnimation(durationMs)
    } else {
      // Issue #879 本轮复核评论修复（问题6）：新状态是静态的——停止旧动画。
      // 字号变更、会话刷新、关闭动画开关或收到 editResult=null 时，
      // acceptUpdate 返回静态帧，但旧 animator 仍在继续每帧写入 visualFrame。
      // 调 stopActiveAnimation 先增 generation 使旧回调失效，再 cancel。
      this.stopActiveAnimation()
    }
  }

  // Issue #879 复核评论修复（问题4）：更新主 Text 的静态裁切 path 命令。
  // 静态模式（非动画）不做裁切——全尺寸矩形 path。
  // 只有动画中且有 motion owner 时才计算遮罩 path。
  // 初始挂载阶段（contentWidth/contentHeight 为 0）也不裁切——用足够大的矩形。
  // 不再用空字符串——空字符串在内容尺寸为零时会构建宽高为 0 的矩形。
  private updateStaticClipCommands(): void {
    if (this.visualFrame === null || !this.visualFrame.isAnimating ||
        !EditorMotionLayer.needsStaticMask(this.visualFrame)) {
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
          this.visualFrame,
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
  }

  // Issue #879 复核评论修复（问题4）：统一所有权交接——动画结束后释放不再需要的源。
  // - 所有运动窗口消失后，旧源可以标记释放
  // - 但保留当前 displayed 版本的源（因为可能下一笔编辑还需要）
  // - pending 版本的源也保留（等待后台排版完成后消费）
  private releaseSourcesAfterMotion(): void {
    const displayedRevision = this.displayedSnapshot?.revision ?? 0
    const pendingRevision = this.pendingRevision
    // 动画结束后不再有运动窗口——activeRunIds 为空集
    // 所有源都不在 activeRunIds 中，但保留 displayed/pending 版本的源
    const emptyRunIds: Set<string> = new Set()
    this.motionSourcePool.releaseUnusedSources(emptyRunIds, displayedRevision, pendingRevision)
    // 清理不再需要的窗口 Controller
    this.motionWindowControllers.clear()
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
      this.visualFrame = this.motionCoordinator.finishMotion()
      this.isMotionAnimating = false
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
      this.visualFrame = this.motionCoordinator.advance(progress)
      // Issue #879 本轮复核评论修复（问题3）：每帧更新主 Text 的静态裁切 path。
      this.updateStaticClipCommands()
    }
    this.motionAnimator.onFinish = (): void => {
      if (currentGen !== this.motionGeneration) {
        return
      }
      this.isMotionAnimating = false
      this.visualFrame = this.motionCoordinator.finishMotion()
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
  private hitTestContent(contentX: number, contentY: number): VisualCaretPosition {
    if (!this.currentSnapshot || !this.renderResult || this.contentWidth <= 0) {
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

    // Fallback: 旧的自计算命中测试
    const snap = this.currentSnapshot
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

  // 当前光标 UTF-16 offset（从 snapshot.cursorByteOffset 转）。
  private currentCursorUtf16(): number {
    if (!this.currentSnapshot) {
      return 0
    }
    return TextOffsetMapper.utf8ToUtf16(this.currentSnapshot.text, this.currentSnapshot.cursorByteOffset)
  }

  // 当前选区 [anchorUtf16, headUtf16]（从 snapshot 的 byte offset 转）。
  private currentSelectionUtf16(): [number, number] {
    if (!this.currentSnapshot) {
      return [0, 0]
    }
    const text = this.currentSnapshot.text
    const anchor = TextOffsetMapper.utf8ToUtf16(text, this.currentSnapshot.selectionAnchor)
    const head = TextOffsetMapper.utf8ToUtf16(text, this.currentSnapshot.selectionHead)
    return [anchor, head]
  }

  // 是否有非空选区（anchor != head）。
  private hasSelection(): boolean {
    if (!this.currentSnapshot) {
      return false
    }
    return this.currentSnapshot.selectionAnchor !== this.currentSnapshot.selectionHead
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
  private dispatchKeyCommand(cmd: SemanticKeyCommand): void {
    if (!this.dispatcher || !this.currentSnapshot) {
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
  private handleMouseEvent(raw: MouseEvent): void {
    if (!this.currentSnapshot || !this.renderResult || !this.dispatcher) {
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
          this.isDragging = true
          // Issue #629 评论5324447292 第1项：setSelection 走 executeDisplaySelection 统一入口。
          // Issue #629 R11 评论5329310563 第2项：成功后查询最终 selection 再通知 IME。
          this.dispatchSelectionAndSyncIme({ kind: 'setSelection', position: hitPos })
        }
        break
      case 'move':
        if (this.isDragging && this.dragAnchor !== null && inContentArea) {
          // 拖动选区：anchor 不变，head 跟随当前位置。传完整 VisualCaretPosition。
          this.dispatchSelectionAndSyncIme({ kind: 'dragSelect', anchor: this.dragAnchor, head: hitPos })
        }
        break
      case 'up':
        if (this.isDragging) {
          this.isDragging = false
          this.dragAnchor = null
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
  private handleTouchEvent(event: TouchEvent): void {
    if (!this.currentSnapshot || !this.renderResult || !this.dispatcher) {
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
        this.isDragging = true
        break
      case TouchType.Move:
        // 拖动选区：anchor 不变，head 跟随当前位置。传完整 VisualCaretPosition。
        if (this.isDragging && this.dragAnchor !== null && inContentArea
          && this.dragAnchor.utf16Offset !== hitPos.utf16Offset) {
          this.dispatchSelectionAndSyncIme({ kind: 'dragSelect', anchor: this.dragAnchor, head: hitPos })
        }
        break
      case TouchType.Up:
        if (this.isDragging) {
          if (inContentArea && this.dragAnchor !== null
            && this.dragAnchor.utf16Offset === hitPos.utf16Offset) {
            // 单击：经 dispatcher 发 selection，成功后查询最终 selection 再通知 IME。
            this.dispatchSelectionAndSyncIme({ kind: 'setSelection', position: hitPos })
          }
          this.isDragging = false
          this.dragAnchor = null
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
        if (this.renderResult && this.currentSnapshot) {
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

          // 3. motion layer — Issue #879 复核评论修复（问题3）：运动层用稳定源池。
          // 当 visualFrame.isAnimating 且 needsMotionLayer 时，渲染运动层 Text + clipShape。
          // 吐字：新字源完整排版（newStyledString），clipShape 限制在 [x0, front] 裁切窗口。
          // 吞字：旧字源完整排版（oldStyledString），clipShape 限制在 [x0, front] 裁切窗口。
          // 源池用 sourceRevision + stableRunId 保存 StyledString/TextController，
          // ForEach key 基于稳定 run 身份（runId），而非一次性自增事务编号。
          if (this.visualFrame !== null && this.visualFrame.isAnimating &&
              EditorMotionLayer.needsMotionLayer(this.visualFrame)) {
            MotionTextOverlayBuilder({
              motionWindowSources: this.buildMotionWindowSources(),
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
          // 动画中：光标位置由 visualFrame 决定（画在裁切前沿 front）。
          // 非动画：光标画在静态位置（caretRect）。
          if (this.visualFrame !== null && this.visualFrame.isAnimating) {
            // 动画中：光标位置由运动协调器的视觉帧决定
            Rect()
              .position({ x: this.visualFrame.caretX, y: this.visualFrame.caretY })
              .width(2)
              .height(this.visualFrame.caretHeight)
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
            onLayoutReady: (token: number, lineLayouts: LineLayout[]): void => {
              this.onPendingLayoutReady(token, lineLayouts)
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
