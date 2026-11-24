// SmoothCaretOverlay — 写作区平滑光标视觉层（Issue #1056）。
//
// 边界：
// - 只渲染一条细长光标矩形，输入是原生 RichEditor 已量好的相对几何（vp）；
// - 用 ArkUI 显式属性动画把上一位置插值到新位置；
// - 视觉层不参与排版、选区、输入法与任何编辑事务，也不写 Core / 不触发保存；
// - 首次展示、切章、失焦重获焦点、远距离跳转/全文替换由上层递增 jumpToken 要求直接定位，
//   普通逐字输入、删除与方向键移动按 durationMs 平滑过渡。

export interface SmoothCaretVisual {
  visible: boolean
  x: number
  y: number
  width: number
  height: number
  // 递增信号：与上次不同表示本次目标直接定位，不做跨屏插值。
  jumpToken: number
}

const DEFAULT_CARET_WIDTH_VP: number = 2
const MIN_CARET_HEIGHT_VP: number = 1
// 合并同一帧内的多次目标写入，只保留一次属性动画。
const CARET_APPLY_DELAY_MS: number = 0

@Component
export struct SmoothCaretOverlay {
  @Prop @Watch('onVisualChanged') visual: SmoothCaretVisual = {
    visible: false,
    x: 0,
    y: 0,
    width: DEFAULT_CARET_WIDTH_VP,
    height: 0,
    jumpToken: 0
  }
  @Prop durationMs: number = 0
  color: ResourceColor = $r('sys.color.ohos_id_color_text_primary')

  @State private renderVisible: boolean = false
  @State private renderX: number = 0
  @State private renderY: number = 0
  @State private renderWidth: number = DEFAULT_CARET_WIDTH_VP
  @State private renderHeight: number = 0

  // 已消费的直接定位信号；只有它变化时才不做插值。
  private appliedJumpToken: number = 0
  private applyTimerId: number = -1

  aboutToDisappear(): void {
    if (this.applyTimerId !== -1) {
      clearTimeout(this.applyTimerId)
      this.applyTimerId = -1
    }
  }

  private onVisualChanged(): void {
    if (!this.visual.visible) {
      // 隐藏必须当帧生效：原生 caretColor 已恢复，不能多留一帧自绘光标。
      if (this.applyTimerId !== -1) {
        clearTimeout(this.applyTimerId)
        this.applyTimerId = -1
      }
      this.applyVisual()
      return
    }
    if (this.visual.jumpToken !== this.appliedJumpToken) {
      // 滚动/远距离跳转：当帧直接随动，不排到定时器之后。
      if (this.applyTimerId !== -1) {
        clearTimeout(this.applyTimerId)
        this.applyTimerId = -1
      }
      this.applyVisual()
      return
    }
    // 父组件可能在同一帧内连续写入多个目标；合并成一次动画，避免堆积定时任务。
    if (this.applyTimerId !== -1) {
      return
    }
    this.applyTimerId = setTimeout(() => {
      this.applyTimerId = -1
      this.applyVisual()
    }, CARET_APPLY_DELAY_MS)
  }

  private applyVisual(): void {
    const target: SmoothCaretVisual = this.visual
    if (!target.visible) {
      // 失焦 / 选区非折叠 / 位置不可获取：立即隐藏并恢复原生光标。
      this.appliedJumpToken = target.jumpToken
      this.renderVisible = false
      return
    }
    const width: number = target.width > 0 ? target.width : DEFAULT_CARET_WIDTH_VP
    const height: number = target.height > 0 ? target.height : MIN_CARET_HEIGHT_VP
    const duration: number = Math.max(0, Math.round(this.durationMs))
    const direct: boolean = !this.renderVisible ||
      target.jumpToken !== this.appliedJumpToken || duration === 0
    this.appliedJumpToken = target.jumpToken
    if (direct) {
      this.renderVisible = true
      this.renderX = target.x
      this.renderY = target.y
      this.renderWidth = width
      this.renderHeight = height
      return
    }
    this.getUIContext().animateTo({
      duration: duration,
      curve: Curve.EaseOut
    }, () => {
      this.renderX = target.x
      this.renderY = target.y
      this.renderWidth = width
      this.renderHeight = height
    })
  }

  build() {
    Stack() {
      if (this.renderVisible && this.renderHeight > 0) {
        Row()
          .width(this.renderWidth)
          .height(this.renderHeight)
          .position({ x: this.renderX, y: this.renderY })
          .backgroundColor(this.color)
          .borderRadius(Math.min(this.renderWidth, 2) / 2)
      }
    }
    .width('100%')
    .height('100%')
    // 裁切在正文可视区域内：光标滚出视口时视觉层不能停留在旧屏幕位置。
    .clip(true)
    .hitTestBehavior(HitTestMode.Transparent)
  }
}
