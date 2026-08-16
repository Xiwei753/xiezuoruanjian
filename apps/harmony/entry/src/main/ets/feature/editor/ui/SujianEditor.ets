// SujianEditor — 写作区组件宿主。
// 消费 EditorLayoutSnapshot 经 EditorRenderBackend 静态渲染；
// 输入经 EditorInputAdapter + SelectionController，IME 连接经 HarmonyImeConnection。
// 用 Stack 组合静态层：selection background → text → composition underline → static caret。
// 不自己维护 cursor timer/animation；光标固定显示，不做闪烁/平滑移动/淡入淡出。
// 动画层以后新增 motion 消费同一份 snapshot，不改 session/input/render 的数据边界。

import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { EditorRenderBackend, EditorRenderResult } from '../render/EditorRenderBackend'
import { SelectionRect, CaretRect, CompositionUnderlineRect } from '../render/editor_render_geometry'
import { HarmonyImeConnection } from '../input/HarmonyImeConnection'
import { EditorInputAdapter } from '../input/EditorInputAdapter'
import { SelectionController } from '../selection/SelectionController'

@Component
export struct SujianEditor {
  @Prop snapshot: EditorLayoutSnapshot | null = null
  @Prop onSelectionTap: ((utf16Offset: number) => void) | null = null
  // IME 连接与输入/选区控制器由父组件注入；SujianEditor 在获得/失去焦点时 attach/detach IME。
  harmonyImeConnection: HarmonyImeConnection | null = null
  inputAdapter: EditorInputAdapter | null = null
  selectionController: SelectionController | null = null
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

  build() {
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
          .onAreaChange((_oldArea: Area, newArea: Area): void => {
            this.containerWidth = Number(newArea.width)
            this.textHeight = Number(newArea.height)
            // 容器尺寸已知后重新计算几何（选区/光标/composition 下划线）。
            this.refreshRenderLayout()
          })
          .onTouch((event: TouchEvent): void => {
            if (event.type === TouchType.Up && this.onSelectionTap && this.snapshot && this.renderResult) {
              const snap = this.snapshot
              const measureUtils = this.getUIContext().getMeasureUtils()
              const measureTextFn = (s: string): number => {
                return measureUtils.measureText({ textContent: s, fontSize: snap.fontSize })
              }
              const touch = event.touches[0]
              const utf16Offset = EditorRenderBackend.hitTestByPoint(
                snap,
                this.containerWidth,
                this.textHeight,
                touch.x,
                touch.y,
                measureTextFn,
              )
              this.onSelectionTap(utf16Offset)
              // 通知 IME 当前选区（光标位置）
              if (this.harmonyImeConnection) {
                this.harmonyImeConnection.notifySelection(utf16Offset, utf16Offset).then(() => {})
              }
            }
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
}
