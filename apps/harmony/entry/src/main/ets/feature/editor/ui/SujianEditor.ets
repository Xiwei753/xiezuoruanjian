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
      if (this.renderResult) {
        // 静态文本渲染（选区/光标/composition 下划线由后续 motion 层叠加）。
        Text(this.renderResult.text)
          .fontSize(16)
          .width('100%')
          .textAlign(TextAlign.Start)
          .onTouch((event: TouchEvent): void => {
            if (event.type === TouchType.Up && this.onSelectionTap && this.renderResult) {
              // 坐标→offset 粗略估算：字符宽度按 fontSize*0.6 近似。
              // 精确转换需要文本布局信息，后续 motion 层接入时改进。
              const fontSize = 16
              const estimatedCharWidth = fontSize * 0.6
              const touchX = event.touches[0].x
              const textLen = this.renderResult.text.length
              let estimatedOffset = Math.round(touchX / estimatedCharWidth)
              estimatedOffset = Math.max(0, Math.min(estimatedOffset, textLen))
              this.onSelectionTap(estimatedOffset)
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
