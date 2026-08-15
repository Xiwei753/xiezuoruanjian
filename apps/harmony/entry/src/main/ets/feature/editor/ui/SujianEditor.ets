// SujianEditor — 写作区组件宿主。
// 消费 EditorLayoutSnapshot 经 EditorRenderBackend 静态渲染；
// 输入经 EditorInputAdapter + SelectionController。
// 不自己维护 cursor timer/animation。

import { EditorLayoutSnapshot } from '../render/EditorLayoutSnapshot'
import { EditorRenderBackend, EditorRenderResult } from '../render/EditorRenderBackend'

@Component
export struct SujianEditor {
  @Prop snapshot: EditorLayoutSnapshot | null = null
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
      } else {
        Text('')
      }
    }
    .width('100%')
    .height('100%')
  }
}
