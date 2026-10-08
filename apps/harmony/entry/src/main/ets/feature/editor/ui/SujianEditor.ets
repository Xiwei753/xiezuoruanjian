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
import { SelectionRect, CaretRect, CompositionUnderlineRect } from '../render/editor_render_geometry'
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
    }
    this.refreshRender()
    this.updateVisualFrame()
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
      if (!this.textMounted || this.currentSnapshot === null) {
        return
      }
      // 到这里 Text 已完成本轮 layout，才读取 LayoutManager。
      this.refreshRenderLayout()
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
  private onPresentationUpdateChanged(): void {
    // 从 presentationUpdate 同步 currentSnapshot
    if (this.presentationUpdate === null) {
      this.currentSnapshot = null
    } else {
      this.currentSnapshot = this.presentationUpdate.layoutSnapshot
    }

    if (this.currentSnapshot === null) {
      // Issue #768 评论5837229456：通过 applyStyledText 清空 StyledString（受 textMounted 守卫）
      this.applyStyledText()
      this.lineResolver.cancelWait()
      this.lineResolver.updateLayout(null)
      this.renderResult = null
      this.selectionRects = []
      this.caretRect = null
      this.compositionUnderlineRects = []
      this.visualFrame = null
      // Issue #768：会话失效时推送诊断快照（hasActiveSession=false）。
      this.pushDiagnosticsSnapshot()
      return
    }
    // 只提交新的文字/样式；布局结果等系统 layout 完成回调后更新。
    this.refreshRender()
    // Issue #879：通过运动协调器接收更新，生成视觉帧。
    this.updateVisualFrame()
    // Issue #768 评论5837824666：推送诊断快照（布局相关字段从缓存读取）。
    // 诊断方可通过 revision !== layoutRevision 判断布局是否已跟上 snapshot。
    this.pushDiagnosticsSnapshot()
  }

  // Issue #879：通过运动协调器接收编辑更新，计算当前视觉帧。
  // 当 presentationUpdate 包含 editResult 时，运动协调器会规划文字运动（吐字/吞字/光标移动等）；
  // 当 editResult 为 null（初始加载/布局重建）时，直接显示静态画面。
  private updateVisualFrame(): void {
    if (this.presentationUpdate === null || this.currentSnapshot === null) {
      this.visualFrame = null
      return
    }
    // 从 renderResult 获取行布局信息，供运动协调器计算字形位置。
    const lineLayouts = this.renderResult?.lines ?? []
    this.visualFrame = this.motionCoordinator.acceptUpdate(
      this.presentationUpdate,
      lineLayouts,
      this.motionPolicy
    )
  }

  // Issue #768 评论5837229456：把"生成 StyledString"和"写进 TextController"分开。
  // API15 之前，在 aboutToAppear 里通过 TextController.setStyledString() 设置内容无法显示，
  // 因为 Text 组件还未创建。用 textMounted 守卫确保只在 Text.onAppear 之后才下发 StyledString。
  private applyStyledText(): void {
    if (!this.textMounted) {
      return
    }
    if (this.currentSnapshot === null) {
      this.textController.setStyledString(new MutableStyledString(''))
      return
    }
    const config: EditorStyleConfig = {
      fontSize: this.currentSnapshot.fontSize,
      lineSpacingMultiplier: this.currentSnapshot.lineSpacingMultiplier,
      autoIndentEnabled: this.currentSnapshot.autoIndentEnabled,
      autoIndentWidth: this.currentSnapshot.autoIndentWidth,
    }
    const styled = EditorTextStyleProjector.projectToStyledString(this.currentSnapshot.text, config)
    this.textController.setStyledString(styled)
  }

  // 基础渲染：用 render() 转 offset，不含几何（lines/rects 为空）。
  // aboutToAppear/aboutToUpdate 调用；onAreaChange 触发后改用 refreshRenderLayout 补几何。
  // Issue #629 评论11 第2项：refreshRender 不再用 render() 的空 lines 覆盖 lineResolver。
  // render() 不计算几何（lines: []），用它覆盖会把 resolver 清空。lineResolver 只在
  // refreshRenderLayout（contentWidth > 0，有真实几何）时更新（带身份）。
  // Issue #768 评论5837229456：不再在 refreshRender 里直接调 setStyledString，
  // 改为通过 applyStyledText()（受 textMounted 守卫）下发 StyledString。
  private refreshRender(): void {
    if (this.currentSnapshot) {
      // 仍然用 EditorRenderBackend.render() 转 offset（不含几何）
      const result = EditorRenderBackend.render(this.currentSnapshot)
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
    // 通过 applyStyledText 下发 StyledString（受 textMounted 守卫）
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
    if (this.currentSnapshot && this.contentWidth > 0) {
      const snap = this.currentSnapshot

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

          // 3. composition underline（在 text 上面）
          ForEach(this.compositionUnderlineRects, (rect: CompositionUnderlineRect, index: number) => {
            Rect()
              .position({ x: rect.x, y: rect.y })
              .width(rect.width)
              .height(rect.height)
              .fill(this.compositionUnderlineColor)
          }, (rect: CompositionUnderlineRect, index: number) => index.toString())

          // 4. caret（最上层）— Issue #879：单一视觉光标。
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
