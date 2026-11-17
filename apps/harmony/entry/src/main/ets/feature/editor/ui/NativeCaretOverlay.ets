// NativeCaretOverlay — 自绘光标覆盖层。
// Issue #1034 评论 #5：保留自绘光标的视觉效果，但位置只从 RichEditorController.getCaretRect()
// 及 onSelectionChange 获取，不再独立命中测量、断行和光标模型。
//
// 规则：
// - 选区非空 / 失焦 / 系统组件无法提供光标矩形时不用自绘层；原生光标作为可用回退。
// - 若用透明原生光标显示自绘版，切到选择态立即恢复 .caretColor(themeColor)，
//   保证系统选择手柄仍可见。
// - 不能让自绘层消费触摸事件。
// - 光标动画单独作用于 overlay 的矩形，不驱动正文重新排版。

@Component
export struct NativeCaretOverlay {
  @Prop caretX: number = 0
  @Prop caretY: number = 0
  @Prop caretWidth: number = 2
  @Prop caretHeight: number = 20
  @Prop caretVisible: boolean = false
  @Prop caretColor: ResourceColor = $r('sys.color.ohos_id_color_text_primary')

  build() {
    if (this.caretVisible) {
      Rect()
        .position({ x: this.caretX, y: this.caretY })
        .width(this.caretWidth)
        .height(this.caretHeight)
        .fill(this.caretColor)
    }
  }
}
