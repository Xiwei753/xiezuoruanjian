// SujianEditor — 写作区组件宿主。
// 消费 EditorLayoutSnapshot 经 EditorRenderBackend 静态渲染；
// 输入经 EditorInputAdapter + SelectionController，IME 连接经 HarmonyImeConnection。
// 用 Stack 组合静态层：selection background → text → composition underline → static caret。
// 不自己维护 cursor timer/animation；光标固定显示，不做闪烁/平滑移动/淡入淡出。
// 动画层以后新增 motion 消费同一份 snapshot，不改 session/input/render 的数据边界。
//
// Issue #629 评论 5 第 5 节：生产接入键盘/鼠标/触控板/手写笔。
// - onKeyEvent → KeyboardInputService.normalize → toSemanticCommand → EditorInputAdapter/SelectionController
// - onMouse → PointerInputService.normalizeMouseEvent → 点击/drag selection/滚轮 viewport
// - onTouch → 真实 drag selection（TouchDown 记 anchor, TouchMove drag, TouchUp 结束）
// - 笔事件由 StylusInputService 识别后按触控选择/书写区交互处理
// 原始平台事件（KeyEvent/MouseEvent/TouchEvent/StylusEvent）不进 Rust，转成语义命令。
//
// Issue #629 评论 5 第 8 节：真实可滚动 viewport。
// - 用 Scroll(scroller) 包裹内容，正文+选区+composition+caret 在同一滚动坐标系里。
// - onScroll 回调把 viewport 状态回调给 WritingScreen（onViewportChange）。
// - restoreViewport(state) 由 WritingScreen 在接续加载完成、layout 就绪后调用。

import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { EditorRenderBackend, EditorRenderResult } from '../render/EditorRenderBackend'
import { SelectionRect, CaretRect, CompositionUnderlineRect, LineLayout } from '../render/editor_render_geometry'
import { HarmonyImeConnection } from '../input/HarmonyImeConnection'
import { EditorInputAdapter } from '../input/EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'
import { TextOffsetMapper } from '../input/TextOffsetMapper'
import { KeyboardInputService, PlatformKeyEvent, SemanticKeyCommand } from '../../../platform/input/KeyboardInputService'
import { PointerInputService, PlatformPointerEvent } from '../../../platform/input/PointerInputService'
import { StylusInputService } from '../../../platform/input/StylusInputService'
import { EditorViewportState } from '../render/EditorViewportState'
import { KeyEvent, MouseEvent } from '@kit.InputKit'

@Component
export struct SujianEditor {
  @Prop snapshot: EditorLayoutSnapshot | null = null
  @Prop onSelectionTap: ((utf16Offset: number) => void) | null = null
  // IME 连接与输入/选区控制器由父组件注入；SujianEditor 在获得/失去焦点时 attach/detach IME。
  harmonyImeConnection: HarmonyImeConnection | null = null
  inputAdapter: EditorInputAdapter | null = null
  selectionController: SelectionController | null = null
  // viewport 状态变化回调（供 WritingScreen 持续写入接续状态）。
  onViewportChange: ((state: EditorViewportState) => void) | null = null
  // 静态渲染颜色（默认值，可由父组件覆盖）。
  selectionBackgroundColor: string = '#33007AFF'
  compositionUnderlineColor: string = '#FF007AFF'
  caretColor: string = '#FF000000'
  @State renderResult: EditorRenderResult | null = null
  // 静态渲染几何（从 renderResult 同步出来，供 build() 直接绑定）。
  @State selectionRects: SelectionRect[] = []
  @State caretRect: CaretRect | null = null
  @State compositionUnderlineRects: CompositionUnderlineRect[] = []
  // 实际容器宽度与文本渲染总高度（px），由 onAreaChange 更新，供命中测试与几何计算。
  @State containerWidth: number = 0
  @State textHeight: number = 0
  // viewport 滚动控制器（ArkUI Scroller）。restoreViewport 用它 scrollTo。
  private scroller: Scroller = new Scroller()
  // 输入服务单例。
  private keyboardService: KeyboardInputService = KeyboardInputService.getInstance()
  private pointerService: PointerInputService = PointerInputService.getInstance()
  private stylusService: StylusInputService = StylusInputService.getInstance()
  // drag selection 状态：鼠标/触摸按下时记 anchor UTF-16 offset，move 时 drag select。
  private dragAnchorUtf16: number = -1
  private isDragging: boolean = false

  aboutToAppear(): void {
    this.refreshRender()
  }

  aboutToUpdate(): void {
    this.refreshRender()
  }

  // 基础渲染：用 render() 转 offset，不含几何（lines/rects 为空）。
  // aboutToAppear/aboutToUpdate 调用；onAreaChange 触发后改用 refreshRenderLayout 补几何。
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
    }
  }

  // 完整静态渲染：用 renderLayout() 计算行布局/选区/光标/composition 下划线几何。
  // 在 onAreaChange 中调用（此时 containerWidth/textHeight 已更新，getUIContext 可用）。
  private refreshRenderLayout(): void {
    if (this.snapshot && this.containerWidth > 0) {
      const snap = this.snapshot
      // 真实文本测量：getMeasureUtils().measureText 返回 px 单行宽度，
      // fontSize 用 snap.fontSize（vp）与 Text.fontSize 一致，测量与渲染同一字体度量。
      const measureUtils = this.getUIContext().getMeasureUtils()
      const measureTextFn = (s: string): number => {
        return measureUtils.measureText({ textContent: s, fontSize: snap.fontSize })
      }
      const result = EditorRenderBackend.renderLayout(
        snap,
        this.containerWidth,
        this.textHeight,
        measureTextFn,
      )
      this.renderResult = result
      this.selectionRects = result.selectionRects
      this.caretRect = result.caretRect
      this.compositionUnderlineRects = result.compositionUnderlineRects
    }
  }

  // 命中测试：给定触摸/鼠标坐标 (px, 相对内容左上)，返回命中的 UTF-16 code unit offset。
  // 坐标已扣除滚动偏移（onTouch/onMouse 给的是相对组件左上的坐标，内容在 Scroll 里滚动，
  // 需要加上 scrollOffset 才是内容坐标系）。但 ArkUI Touch/Mouse 事件的 x/y 是相对组件左上，
  // Scroll 滚动后内容偏移，hitTestByPoint 期望内容坐标系坐标。
  // 此处用 scroller.currentOffset() 获取当前滚动偏移，把事件坐标加上偏移转成内容坐标。
  private hitTestContent(contentX: number, contentY: number): number {
    if (!this.snapshot || !this.renderResult || this.containerWidth <= 0) {
      return 0
    }
    const snap = this.snapshot
    const measureUtils = this.getUIContext().getMeasureUtils()
    const measureTextFn = (s: string): number => {
      return measureUtils.measureText({ textContent: s, fontSize: snap.fontSize })
    }
    return EditorRenderBackend.hitTestByPoint(
      snap,
      this.containerWidth,
      this.textHeight,
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

  // 找当前光标所在行号（用 renderResult.lines）。返回 -1 表示无行信息。
  private currentLineIndex(): number {
    if (!this.renderResult || this.renderResult.lines.length === 0) {
      return -1
    }
    const cursor = this.currentCursorUtf16()
    const lines: LineLayout[] = this.renderResult.lines
    for (let i = 0; i < lines.length; i++) {
      if (cursor >= lines[i].startUtf16 && cursor <= lines[i].endUtf16) {
        return i
      }
    }
    // 越界时归最后一行。
    return lines.length - 1
  }

  // ── 键盘语义命令分发 ──
  // 把 SemanticKeyCommand 转成对 EditorInputAdapter/SelectionController 的调用。
  // 原始 KeyEvent 不进 Rust；这里只调语义命令。
  private dispatchKeyCommand(cmd: SemanticKeyCommand): void {
    if (!this.inputAdapter || !this.selectionController || !this.snapshot) {
      return
    }
    const text: string = this.snapshot.text
    const cursor: number = this.currentCursorUtf16()
    const sel: [number, number] = this.currentSelectionUtf16()
    const selStart: number = Math.min(sel[0], sel[1])
    const selEnd: number = Math.max(sel[0], sel[1])
    switch (cmd.kind) {
      case 'character':
        // 有选区时替换选区（子代理1负责 EditorInputAdapter 内部选区感知，
        // 这里传当前光标位置；子代理1改后内部会判断选区替换）。
        this.inputAdapter.onTextInput(cmd.text, cursor).then(() => {})
        break
      case 'enter':
        this.inputAdapter.onTextInput('\n', cursor).then(() => {})
        break
      case 'tab':
        this.inputAdapter.onTextInput('\t', cursor).then(() => {})
        break
      case 'backspace':
        if (this.hasSelection()) {
          // 有选区：删除整个选区。
          this.inputAdapter.onDelete(selStart, selEnd).then(() => {})
        } else if (cursor > 0) {
          // 空选区：删除光标前一个字符。
          this.inputAdapter.onDelete(cursor - 1, cursor).then(() => {})
        }
        break
      case 'delete':
        if (this.hasSelection()) {
          this.inputAdapter.onDelete(selStart, selEnd).then(() => {})
        } else if (cursor < text.length) {
          // 空选区：删除光标后一个字符。
          this.inputAdapter.onDelete(cursor, cursor + 1).then(() => {})
        }
        break
      case 'arrowLeft':
        if (cmd.extend) {
          // shift+arrowLeft：保持 anchor，head 左移。
          this.selectionController.onDragSelect(sel[0], Math.max(0, sel[1] - 1)).then(() => {})
        } else {
          // 空选区时光标左移；有选区时光标移到选区开头（collapse）。
          if (this.hasSelection()) {
            this.selectionController.onTap(selStart).then(() => {})
          } else {
            this.selectionController.onTap(Math.max(0, cursor - 1)).then(() => {})
          }
        }
        break
      case 'arrowRight':
        if (cmd.extend) {
          this.selectionController.onDragSelect(sel[0], Math.min(text.length, sel[1] + 1)).then(() => {})
        } else {
          if (this.hasSelection()) {
            this.selectionController.onTap(selEnd).then(() => {})
          } else {
            this.selectionController.onTap(Math.min(text.length, cursor + 1)).then(() => {})
          }
        }
        break
      case 'arrowUp': {
        // 移到上一行开头（简化：不做列保持）。
        const lineIdx = this.currentLineIndex()
        if (lineIdx > 0 && this.renderResult) {
          const targetLine = this.renderResult.lines[lineIdx - 1]
          if (cmd.extend) {
            this.selectionController.onDragSelect(sel[0], targetLine.startUtf16).then(() => {})
          } else {
            this.selectionController.onTap(targetLine.startUtf16).then(() => {})
          }
        }
        break
      }
      case 'arrowDown': {
        // 移到下一行开头。
        const lineIdx = this.currentLineIndex()
        if (lineIdx >= 0 && this.renderResult && lineIdx < this.renderResult.lines.length - 1) {
          const targetLine = this.renderResult.lines[lineIdx + 1]
          if (cmd.extend) {
            this.selectionController.onDragSelect(sel[0], targetLine.startUtf16).then(() => {})
          } else {
            this.selectionController.onTap(targetLine.startUtf16).then(() => {})
          }
        }
        break
      }
      case 'home': {
        // 行首（当前行 startUtf16）；无行信息时归 0。
        const lineIdx = this.currentLineIndex()
        const target: number = lineIdx >= 0 && this.renderResult
          ? this.renderResult.lines[lineIdx].startUtf16
          : 0
        if (cmd.extend) {
          this.selectionController.onDragSelect(sel[0], target).then(() => {})
        } else {
          this.selectionController.onTap(target).then(() => {})
        }
        break
      }
      case 'end': {
        // 行末（当前行 endUtf16）；无行信息时归 text.length。
        const lineIdx = this.currentLineIndex()
        const target: number = lineIdx >= 0 && this.renderResult
          ? this.renderResult.lines[lineIdx].endUtf16
          : text.length
        if (cmd.extend) {
          this.selectionController.onDragSelect(sel[0], target).then(() => {})
        } else {
          this.selectionController.onTap(target).then(() => {})
        }
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
  // 点击命中、按下拖动选区、抬起结束选择走 SelectionController；滚轮由 Scroll 组件自动消费。
  private handleMouseEvent(raw: MouseEvent): void {
    if (!this.snapshot || !this.renderResult) {
      return
    }
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
          // 先设光标（单击位置）；move 时改 drag select。
          if (this.selectionController) {
            this.selectionController.onTap(hitUtf16).then(() => {})
          }
        }
        break
      case 'move':
        if (this.isDragging && this.selectionController) {
          // 拖动选区：anchor 不变，head 跟随当前位置。
          this.selectionController.onDragSelect(this.dragAnchorUtf16, hitUtf16).then(() => {})
        }
        break
      case 'up':
        if (this.isDragging) {
          // 抬起结束 drag。如果 anchor == head（单击），onTap 已在 down 时调用；
          // 如果 anchor != head（拖动选区），onDragSelect 已在 move 时调用。
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
  private handleTouchEvent(event: TouchEvent): void {
    if (!this.snapshot || !this.renderResult) {
      return
    }
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
        // 拖动选区：anchor 不变，head 跟随当前位置。
        if (this.isDragging && this.selectionController && this.dragAnchorUtf16 !== hitUtf16) {
          this.selectionController.onDragSelect(this.dragAnchorUtf16, hitUtf16).then(() => {})
        }
        break
      case TouchType.Up:
        if (this.isDragging) {
          if (this.dragAnchorUtf16 === hitUtf16) {
            // 单击（down 和 up 同位置）：设光标。
            if (this.onSelectionTap) {
              this.onSelectionTap(hitUtf16)
            }
            if (this.selectionController) {
              this.selectionController.onTap(hitUtf16).then(() => {})
            }
            // 通知 IME 当前选区（光标位置）
            if (this.harmonyImeConnection) {
              this.harmonyImeConnection.notifySelection(hitUtf16, hitUtf16).then(() => {})
            }
          }
          // 拖动选区（anchor != head）已在 Move 时通过 onDragSelect 设置，Up 时保持。
          this.isDragging = false
          this.dragAnchorUtf16 = -1
        }
        break
      default:
        break
    }
  }

  // viewport 状态变化时回调给 WritingScreen。
  private emitViewportChange(xOffset: number, yOffset: number): void {
    if (!this.onViewportChange) {
      return
    }
    const state: EditorViewportState = new EditorViewportState(
      yOffset,
      xOffset,
      this.containerWidth,
      this.textHeight > 0 ? this.textHeight : 0,
      this.containerWidth,
      this.textHeight
    )
    this.onViewportChange(state)
  }

  // 接续恢复：由 WritingScreen 在接续加载完成、layout 就绪后调用。
  // 把 viewport 滚动到 state 指定的 offset。clamp 防止越界。
  restoreViewport(state: EditorViewportState): void {
    const clamped: EditorViewportState = state.clamped()
    this.scroller.scrollTo({ xOffset: clamped.scrollLeft, yOffset: clamped.scrollTop })
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
              // 编辑框获得焦点：attach 自绘编辑框到 IME Kit
              if (this.harmonyImeConnection && this.inputAdapter && this.selectionController) {
                this.harmonyImeConnection.attach(this.inputAdapter, this.selectionController).then(() => {})
              }
            })
            .onBlur((): void => {
              // 编辑框失焦：detach IME Kit
              if (this.harmonyImeConnection) {
                this.harmonyImeConnection.detach().then(() => {})
              }
            })
            .onKeyEvent((event: KeyEvent): boolean => {
              // 物理键盘事件 → KeyboardInputService 归一化 → 语义命令 → EditorInputAdapter/SelectionController。
              // 原始 KeyEvent 不进 Rust。
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
              this.containerWidth = Number(newArea.width)
              this.textHeight = Number(newArea.height)
              // 容器尺寸已知后重新计算几何（选区/光标/composition 下划线）。
              this.refreshRenderLayout()
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
      .height('100%')
    }
    .scrollable(ScrollDirection.Vertical)
    .scrollBar(BarState.Auto)
    .width('100%')
    .height('100%')
    .onScroll((xOffset: number, yOffset: number): void => {
      // 滚动时把 viewport 状态回调给 WritingScreen（供接续状态持续写入）。
      this.emitViewportChange(xOffset, yOffset)
    })
  }
}
