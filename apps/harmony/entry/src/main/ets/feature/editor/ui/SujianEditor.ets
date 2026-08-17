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
import { EditorSemanticDispatcher } from '../input/EditorSemanticDispatcher'
import { LineNavigationResolver, LineLayoutState, NavigationLine, LineCaretStop } from '../selection/LineNavigationResolver'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { KeyboardInputService, PlatformKeyEvent, SemanticKeyCommand } from '../../../platform/input/KeyboardInputService'
import { PointerInputService, PlatformPointerEvent } from '../../../platform/input/PointerInputService'
import { StylusInputService } from '../../../platform/input/StylusInputService'
import { EditorViewportState } from '../render/EditorViewportState'
import { EditorViewportController, ViewportDimensions } from '../render/EditorViewportController'
import { KeyEvent, MouseEvent } from '@kit.InputKit'

@Component
export struct SujianEditor {
  @Prop @Watch('onSnapshotChanged') snapshot: EditorLayoutSnapshot | null = null
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
  // 静态渲染颜色（默认值，可由父组件覆盖）。
  selectionBackgroundColor: string = '#33007AFF'
  compositionUnderlineColor: string = '#FF007AFF'
  caretColor: string = '#FF000000'
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
  private scroller: Scroller = new Scroller()
  // 输入服务单例。
  private keyboardService: KeyboardInputService = KeyboardInputService.getInstance()
  private pointerService: PointerInputService = PointerInputService.getInstance()
  private stylusService: StylusInputService = StylusInputService.getInstance()
  // Issue #629 评论11 第2项：行级导航解析器。
  // SujianEditor 持有 renderResult，每次渲染结果更新后同步给 resolver。
  // Dispatcher 行导航命令出队时通过 resolver 读取最新行布局计算目标位置。
  private lineResolver: LineNavigationResolver = new LineNavigationResolver()
  // drag selection 状态：鼠标/触摸按下时记 anchor UTF-16 offset，move 时 drag select。
  private dragAnchorUtf16: number = -1
  private isDragging: boolean = false

  aboutToAppear(): void {
    this.refreshRender()
    // Issue #629 评论11 第2项：注册行级导航解析器到 Dispatcher。
    // SujianEditor 持有 renderResult；每次渲染更新后同步给 resolver。
    // Dispatcher 行导航命令出队时通过 resolver 读最新行布局计算目标。
    if (this.dispatcher) {
      this.dispatcher.registerLineResolver(this.lineResolver)
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
            contentHeight: this.contentHeight,
          }
        }
      )
    }
  }

  aboutToDisappear(): void {
    // Issue #629 评论11 第2项 + 评论15 第3项：取消行导航等待 + 注销行级导航解析器。
    this.lineResolver.cancelWait()
    if (this.dispatcher) {
      this.dispatcher.unregisterLineResolver()
    }
    // Issue #629 评论6 Part C：注销父子 viewport 恢复通道。
    if (this.viewportController) {
      this.viewportController.unregister()
    }
  }

  // Issue #629 评论16 第1项：用 @Watch 监听 snapshot 变化替代 aboutToUpdate。
  // aboutToUpdate 不是 OpenHarmony 官方生命周期回调（官方 V1 监听机制是 @Watch）。
  // snapshot 前进时先 refreshRender（无几何 fallback），再 refreshRenderLayout
  // （contentWidth > 0 时用已有宽度计算真实 lines + 更新 lineResolver 带身份）。
  // 这样打完字后按 Up/Down/Home/End 不需要等 onAreaChange 才更新 lineResolver，
  // resolver 的身份与当前 Core snapshot 匹配，executeLineNavigation 不会 STALE_LAYOUT。
  private onSnapshotChanged(): void {
    if (this.snapshot === null) {
      this.lineResolver.cancelWait()
      this.lineResolver.updateLayout(null)
      this.renderResult = null
      this.selectionRects = []
      this.caretRect = null
      this.compositionUnderlineRects = []
      return
    }
    this.refreshRender()
    this.refreshRenderLayout()
  }

  // 基础渲染：用 render() 转 offset，不含几何（lines/rects 为空）。
  // aboutToAppear/aboutToUpdate 调用；onAreaChange 触发后改用 refreshRenderLayout 补几何。
  // Issue #629 评论11 第2项：refreshRender 不再用 render() 的空 lines 覆盖 lineResolver。
  // render() 不计算几何（lines: []），用它覆盖会把 resolver 清空。lineResolver 只在
  // refreshRenderLayout（contentWidth > 0，有真实几何）时更新（带身份）。
  private refreshRender(): void {
    if (this.snapshot) {
      const result = EditorRenderBackend.render(this.snapshot)
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
  }

  // 完整静态渲染：用 renderLayout() 计算行布局/选区/光标/composition 下划线几何。
  // 在 onAreaChange 中调用（此时 contentWidth/contentHeight 已更新，getUIContext 可用）。
  // Issue #629 评论11 第2项：aboutToUpdate 也调此方法（contentWidth 已 > 0 时），
  // 保证 snapshot/revision/composition 变化时立刻用已有宽度计算真实 lines 并更新 lineResolver，
  // 不等 onAreaChange 才更新。lineResolver 带身份（revision + displayText + cursorUtf16），
  // executeLineNavigation 出队时验证身份匹配才用 lines 算目标。
  private refreshRenderLayout(): void {
    if (this.snapshot && this.contentWidth > 0) {
      const snap = this.snapshot
      // 真实文本测量：getMeasureUtils().measureText 返回 px 单行宽度，
      // fontSize 用 snap.fontSize（vp）与 Text.fontSize 一致，测量与渲染同一字体度量。
      const measureUtils = this.getUIContext().getMeasureUtils()
      const measureTextFn = (s: string): number => {
        return measureUtils.measureText({ textContent: s, fontSize: snap.fontSize })
      }
      const result = EditorRenderBackend.renderLayout(
        snap,
        this.contentWidth,
        this.contentHeight,
        measureTextFn,
      )
      this.renderResult = result
      this.selectionRects = result.selectionRects
      this.caretRect = result.caretRect
      this.compositionUnderlineRects = result.compositionUnderlineRects
      // Issue #629 评论11 第2项 + 评论14 第3项 + 评论15 第3项：行布局更新后同步给行级导航解析器（带身份）。
      // 身份来自 Core 编辑状态：revision + generation + compositionGeneration + displayText。
      // contentWidth/fontSize 决定折行但不属于编辑状态身份（评论15 第2项）。
      // executeLineNavigation 出队时验证此身份与当前 Core snapshot 匹配才用 lines 算目标。
      // Issue #629 评论16 第2项：为每行生成 caretStops（code point 边界 + x 坐标），
      // 供 Up/Down 保持横向位置、鼠标 hit-test 复用同一份算法。
      const navigationLines: NavigationLine[] = []
      for (const ll of result.lines) {
        const stops: LineCaretStop[] = []
        // 逐 code point 边界计算 x 坐标
        let i = ll.startUtf16
        while (i <= ll.endUtf16) {
          const x = measureTextFn(result.text.substring(ll.startUtf16, i))
          stops.push({ utf16Offset: i, x: x })
          // 下一个 code point 边界（不切断 surrogate pair）
          if (i >= result.text.length) {
            break
          }
          const code = result.text.charCodeAt(i)
          if (code >= 0xD800 && code <= 0xDBFF && i + 1 < result.text.length) {
            const low = result.text.charCodeAt(i + 1)
            if (low >= 0xDC00 && low <= 0xDFFF) {
              i += 2
              continue
            }
          }
          i += 1
        }
        navigationLines.push({
          startUtf16: ll.startUtf16,
          endUtf16: ll.endUtf16,
          y: ll.y,
          height: ll.height,
          caretStops: stops
        })
      }
      const layoutState: LineLayoutState = {
        revision: snap.revision,
        generation: snap.generation,
        compositionGeneration: snap.compositionGeneration,
        displayText: snap.text,
        contentWidth: this.contentWidth,
        fontSize: snap.fontSize,
        lines: navigationLines
      }
      this.lineResolver.updateLayout(layoutState)
    }
  }

  // 命中测试：给定触摸/鼠标坐标 (px, 相对内容左上)，返回命中的 UTF-16 code unit offset。
  // 坐标已扣除滚动偏移（onTouch/onMouse 给的是相对组件左上的坐标，内容在 Scroll 里滚动，
  // 需要加上 scrollOffset 才是内容坐标系）。但 ArkUI Touch/Mouse 事件的 x/y 是相对组件左上，
  // Scroll 滚动后内容偏移，hitTestByPoint 期望内容坐标系坐标。
  // 此处用 scroller.currentOffset() 获取当前滚动偏移，把事件坐标加上偏移转成内容坐标。
  private hitTestContent(contentX: number, contentY: number): number {
    if (!this.snapshot || !this.renderResult || this.contentWidth <= 0) {
      return 0
    }
    const snap = this.snapshot
    const measureUtils = this.getUIContext().getMeasureUtils()
    const measureTextFn = (s: string): number => {
      return measureUtils.measureText({ textContent: s, fontSize: snap.fontSize })
    }
    return EditorRenderBackend.hitTestByPoint(
      snap,
      this.contentWidth,
      this.contentHeight,
      contentX,
      contentY,
      measureTextFn,
    )
  }

  // 把视口内坐标（相对组件左上）转成内容坐标（加上滚动偏移）。
  private viewportToContent(viewportX: number, viewportY: number): [number, number] {
    const offset = this.scroller.currentOffset()
    return [viewportX + offset.xOffset, viewportY + offset.yOffset]
  }

  // 当前光标 UTF-16 offset（从 snapshot.cursorByteOffset 转）。
  private currentCursorUtf16(): number {
    if (!this.snapshot) {
      return 0
    }
    return TextOffsetMapper.utf8ToUtf16(this.snapshot.text, this.snapshot.cursorByteOffset)
  }

  // 当前选区 [anchorUtf16, headUtf16]（从 snapshot 的 byte offset 转）。
  private currentSelectionUtf16(): [number, number] {
    if (!this.snapshot) {
      return [0, 0]
    }
    const text = this.snapshot.text
    const anchor = TextOffsetMapper.utf8ToUtf16(text, this.snapshot.selectionAnchor)
    const head = TextOffsetMapper.utf8ToUtf16(text, this.snapshot.selectionHead)
    return [anchor, head]
  }

  // 是否有非空选区（anchor != head）。
  private hasSelection(): boolean {
    if (!this.snapshot) {
      return false
    }
    return this.snapshot.selectionAnchor !== this.snapshot.selectionHead
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
    if (!this.dispatcher || !this.snapshot) {
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
  // 点击命中、按下拖动选区、抬起结束选择经 dispatcher 派发；滚轮由 Scroll 组件自动消费。
  // Issue #629 评论6 Part A：所有输入经 dispatcher 统一排队，不再直接调 SelectionController。
  private handleMouseEvent(raw: MouseEvent): void {
    if (!this.snapshot || !this.renderResult || !this.dispatcher) {
      return
    }
    const dispatcher: EditorSemanticDispatcher = this.dispatcher
    const ev: PlatformPointerEvent = this.pointerService.normalizeMouseEvent(raw)
    // 把视口坐标转成内容坐标。
    const contentCoord: [number, number] = this.viewportToContent(ev.x, ev.y)
    const hitUtf16: number = this.hitTestContent(contentCoord[0], contentCoord[1])
    switch (ev.action) {
      case 'down':
        if (ev.button === 0) {
          // 左键按下：记 anchor，开始 potential drag。
          this.dragAnchorUtf16 = hitUtf16
          this.isDragging = true
          // 先设光标（单击位置）；move 时改 drag select。经 dispatcher 派发保证串行。
          dispatcher.dispatchFireAndForget({ kind: 'setSelection', utf16Offset: hitUtf16 })
        }
        break
      case 'move':
        if (this.isDragging) {
          // 拖动选区：anchor 不变，head 跟随当前位置。经 dispatcher 派发。
          dispatcher.dispatchFireAndForget({ kind: 'dragSelect', utf16Anchor: this.dragAnchorUtf16, utf16Head: hitUtf16 })
        }
        break
      case 'up':
        if (this.isDragging) {
          // 抬起结束 drag。如果 anchor == head（单击），setSelection 已在 down 时派发；
          // 如果 anchor != head（拖动选区），dragSelect 已在 move 时派发。
          this.isDragging = false
          this.dragAnchorUtf16 = -1
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
  // 笔事件（toolType != 0）由 StylusInputService 识别后按触控选择/书写区交互处理。
  // Issue #629 评论6 Part A+D：所有输入经 dispatcher 统一排队；删除重复 onSelectionTap prop（双 owner）。
  // TouchUp 只经 dispatcher 发一次 selection（setSelection），完成后通知 IME。
  private handleTouchEvent(event: TouchEvent): void {
    if (!this.snapshot || !this.renderResult || !this.dispatcher) {
      return
    }
    const dispatcher: EditorSemanticDispatcher = this.dispatcher
    const touch = event.touches[0]
    // 把视口坐标转成内容坐标。
    const contentCoord: [number, number] = this.viewportToContent(touch.x, touch.y)
    const hitUtf16: number = this.hitTestContent(contentCoord[0], contentCoord[1])

    // 笔事件识别：toolType 非 0（TIP=0 是手指，ERASER=1 是橡皮擦，其他是笔尖）。
    // 由 StylusInputService 归一化后按触控选择/书写区交互处理，原始 stylus event 不进 Core。
    const isStylus: boolean = touch.toolType !== undefined && touch.toolType !== 0
    if (isStylus) {
      this.stylusService.normalizeStylusEvent(event)
      // 笔事件按触控选择/书写区交互处理（与触控同一套 hit test + selection）。
    }

    switch (event.type) {
      case TouchType.Down:
        // 按下：记 anchor，开始 potential drag。
        this.dragAnchorUtf16 = hitUtf16
        this.isDragging = true
        break
      case TouchType.Move:
        // 拖动选区：anchor 不变，head 跟随当前位置。经 dispatcher 派发。
        if (this.isDragging && this.dragAnchorUtf16 !== hitUtf16) {
          dispatcher.dispatchFireAndForget({ kind: 'dragSelect', utf16Anchor: this.dragAnchorUtf16, utf16Head: hitUtf16 })
        }
        break
      case TouchType.Up:
        if (this.isDragging) {
          if (this.dragAnchorUtf16 === hitUtf16) {
            // 单击（down 和 up 同位置）：只经 dispatcher 发一次 selection（Part D：删除重复 onSelectionTap）。
            dispatcher.dispatchFireAndForget({ kind: 'setSelection', utf16Offset: hitUtf16 })
            // 通知 IME 当前选区（光标位置）
            if (this.harmonyImeConnection) {
              this.harmonyImeConnection.notifySelection(hitUtf16, hitUtf16).then(() => {})
            }
          }
          // 拖动选区（anchor != head）已在 Move 时通过 dragSelect 派发，Up 时保持。
          this.isDragging = false
          this.dragAnchorUtf16 = -1
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
    const state: EditorViewportState = new EditorViewportState(
      yOffset,
      xOffset,
      this.viewportWidth,
      this.viewportHeight,
      this.contentWidth,
      this.contentHeight
    )
    this.onViewportChange(state)
  }

  // Issue #629 评论6 Part C：Stack 高度：至少 viewport 高度，随正文增长。
  // 旧实现固定 .height('100%')，长正文真实内容高度被压成视口高度，maxScrollTop 永远 0。
  // 新实现：contentHeight > viewportHeight 时用 contentHeight（长正文可滚动）；
  // 否则用 viewportHeight（短正文至少占满视口）；都是 0 时用 '100%' 兜底避免高度 0。
  private stackHeight(): number | string {
    if (this.contentHeight > 0 && this.contentHeight > this.viewportHeight) {
      return this.contentHeight
    }
    if (this.viewportHeight > 0) {
      return this.viewportHeight
    }
    return '100%'
  }

  build() {
    // Scroll 包裹内容，实现真实可滚动 viewport。
    // 正文+选区+composition+caret 在同一滚动坐标系里（Stack align(TopStart)）。
    // 滚轮由 Scroll 自动消费；onScroll 回调把 viewport 状态回调给 WritingScreen。
    Scroll(this.scroller) {
      // Stack 层叠组合静态层：selection background → text → composition underline → static caret。
      // align(TopStart) 让 Text 从左上角开始渲染，与 Rect.position 的坐标系（相对左上）一致。
      Stack() {
        if (this.renderResult && this.snapshot) {
          // 1. selection background（最底层）
          ForEach(this.selectionRects, (rect: SelectionRect, index: number) => {
            Rect()
              .position(rect.x, rect.y)
              .width(rect.width)
              .height(rect.height)
              .fill(this.selectionBackgroundColor)
          }, (rect: SelectionRect, index: number) => index.toString())

          // 2. text
          // wordBreak(BREAK_ALL) 与 EditorRenderBackend.hitTestByPoint 的逐 code point 折行一致。
          Text(this.renderResult.text)
            .fontSize(this.snapshot.fontSize)
            .width('100%')
            .textAlign(TextAlign.Start)
            .wordBreak(WordBreak.BREAK_ALL)
            .focusable(true)
            .onFocus((): void => {
              // 编辑框获得焦点：attach 自绘编辑框到 IME Kit（传 dispatcher，所有输入经 dispatcher 统一排队）
              if (this.harmonyImeConnection && this.dispatcher) {
                this.harmonyImeConnection.attach(this.dispatcher).then(() => {})
              }
            })
            .onBlur((): void => {
              // 编辑框失焦：detach IME Kit
              if (this.harmonyImeConnection) {
                this.harmonyImeConnection.detach().then(() => {})
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
            .onAreaChange((_oldArea: Area, newArea: Area): void => {
              // Issue #629 评论6 Part C：Text onAreaChange 更新的是内层正文实际渲染尺寸
              // （contentWidth/contentHeight），不是外层 Scroll 可视区域尺寸。
              this.contentWidth = Number(newArea.width)
              this.contentHeight = Number(newArea.height)
              // 内容尺寸已知后重新计算几何（选区/光标/composition 下划线）。
              this.refreshRenderLayout()
              // Issue #629 评论7 第4项：内容尺寸更新后通知 controller 重试消费 pending。
              // register 时尺寸可能为 0，pending 保留；onAreaChange 拿到真实尺寸后才消费。
              if (this.viewportController) {
                this.viewportController.notifyDimensionsChanged()
              }
            })
            .onTouch((event: TouchEvent): void => {
              // 触摸/笔事件 → hit test + drag selection。
              this.handleTouchEvent(event)
            })

          // 3. composition underline（在 text 上面）
          ForEach(this.compositionUnderlineRects, (rect: CompositionUnderlineRect, index: number) => {
            Rect()
              .position(rect.x, rect.y)
              .width(rect.width)
              .height(rect.height)
              .fill(this.compositionUnderlineColor)
          }, (rect: CompositionUnderlineRect, index: number) => index.toString())

          // 4. static caret（最上层，不闪烁/不平滑移动）
          if (this.caretRect) {
            Rect()
              .position(this.caretRect.x, this.caretRect.y)
              .width(this.caretRect.width)
              .height(this.caretRect.height)
              .fill(this.caretColor)
          }
        } else {
          Text('')
        }
      }
      .align(Alignment.TopStart)
      .width('100%')
      // Issue #629 评论6 Part C：Stack 高度：至少 viewport 高度，随正文增长。
      // 旧实现固定 .height('100%')，长正文真实内容高度被压成视口高度，maxScrollTop 永远 0。
      // 新实现：contentHeight > viewportHeight 时用 contentHeight（长正文可滚动）；
      // 否则用 viewportHeight（短正文至少占满视口）；都是 0 时用 '100%' 兜底避免高度 0。
      .height(this.stackHeight())
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
    })
    .onScroll((xOffset: number, yOffset: number): void => {
      // 滚动时把 viewport 状态回调给 WritingScreen（供接续状态持续写入）。
      this.emitViewportChange(xOffset, yOffset)
    })
  }
}
