// SujianEditor — 写作区组件宿主。
// 消费 EditorLayoutSnapshot 经 EditorRenderBackend 静态渲染；
// 输入经 EditorInputAdapter + SelectionController。
// 不自己维护 cursor timer/animation。

import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { EditorRenderBackend, EditorRenderResult } from '../render/EditorRenderBackend'

@Component
export struct SujianEditor {
  @Prop snapshot: EditorLayoutSnapshot | null = null
  @Prop onSelectionTap: ((utf16Offset: number) => void) | null = null
  @State renderResult: EditorRenderResult | null = null
  // 实际容器宽度与文本渲染总高度（px），由 onAreaChange 更新，供命中测试计算行高与折行。
  @State containerWidth: number = 0
  @State textHeight: number = 0

  aboutToAppear(): void {
    this.refreshRender()
  }

  aboutToUpdate(): void {
    this.refreshRender()
  }

  private refreshRender(): void {
    if (this.snapshot) {
      this.renderResult = EditorRenderBackend.render(this.snapshot)
    } else {
      this.renderResult = null
    }
  }

  build() {
    Column() {
      if (this.renderResult && this.snapshot) {
        // 静态文本渲染。选区/光标/composition 下划线由 motion 层叠加
        // （Issue #629 第 5 节：先做静态，动画层新增 motion 消费同一份 snapshot，不改输入层/session 层）。
        // wordBreak(BREAK_ALL) 与 EditorRenderBackend.hitTestByPoint 的逐 code point 折行一致。
        Text(this.renderResult.text)
          .fontSize(this.snapshot.fontSize)
          .width('100%')
          .textAlign(TextAlign.Start)
          .wordBreak(WordBreak.BREAK_ALL)
          .onAreaChange((_oldArea: Area, newArea: Area): void => {
            this.containerWidth = Number(newArea.width)
            this.textHeight = Number(newArea.height)
          })
          .onTouch((event: TouchEvent): void => {
            if (event.type === TouchType.Up && this.onSelectionTap && this.snapshot && this.renderResult) {
              const snap = this.snapshot
              // 真实文本测量：getMeasureUtils().measureText 返回 px 单行宽度，
              // fontSize 用 snap.fontSize（vp）与 Text.fontSize 一致，测量与渲染同一字体度量。
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
            }
          })
      } else {
        Text('')
      }
    }
    .width('100%')
    .height('100%')
  }
}
