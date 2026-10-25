// GripAdaptiveToggleRow — 设置行中的握持侧自适应开关。
// 保持单个 Toggle 实例，用实测行宽和开关宽度计算左右锚点；切换握持侧时只动画数值偏移。

import { GripLayoutSide } from '../adaptive/GripLayoutSide'

const GRIP_TOGGLE_SWITCH_DURATION_MS: number = 280
const GRIP_TOGGLE_LABEL_GAP_VP: number = 12

@ComponentV2
export struct GripAdaptiveToggleRow {
  @Param side: GripLayoutSide = 'right'
  @Param isOn: boolean = false
  @Param enabled: boolean = true
  @Param onChange: (isOn: boolean) => void = (_isOn: boolean) => {}

  @Builder
  emptyText() {}

  @Builder
  emptyDescription() {}

  @BuilderParam textBuilder: () => void = this.emptyText
  @BuilderParam descriptionBuilder: () => void = this.emptyDescription

  @Local private rowWidth: number = 0
  @Local private toggleWidth: number = 0
  @Local private toggleOffsetX: number = 0
  private layoutInitialized: boolean = false

  private offsetForSide(side: GripLayoutSide): number {
    if (side === 'left' && this.rowWidth > 0 && this.toggleWidth > 0) {
      return -Math.max(0, this.rowWidth - this.toggleWidth)
    }
    return 0
  }

  private updateRowWidth(area: Area): void {
    const measuredWidth: number = Number(area.width)
    if (measuredWidth <= 0 || measuredWidth === this.rowWidth) {
      return
    }
    this.rowWidth = measuredWidth
    this.toggleOffsetX = this.offsetForSide(this.side)
    this.layoutInitialized = true
  }

  private updateToggleWidth(area: Area): void {
    const measuredWidth: number = Number(area.width)
    if (measuredWidth <= 0 || measuredWidth === this.toggleWidth) {
      return
    }
    this.toggleWidth = measuredWidth
    this.toggleOffsetX = this.offsetForSide(this.side)
    this.layoutInitialized = true
  }

  @Monitor('side')
  onGripSideChanged(): void {
    const targetOffset: number = this.offsetForSide(this.side)
    if (!this.layoutInitialized || this.rowWidth <= 0 || this.toggleWidth <= 0) {
      this.toggleOffsetX = targetOffset
      return
    }
    this.getUIContext().animateTo({
      duration: GRIP_TOGGLE_SWITCH_DURATION_MS,
      curve: Curve.EaseInOut
    }, () => {
      this.toggleOffsetX = targetOffset
    })
  }

  build() {
    Row() {
      Column({ space: 2 }) {
        this.textBuilder()
        this.descriptionBuilder()
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)
      .padding({
        left: this.side === 'left' ? this.toggleWidth + GRIP_TOGGLE_LABEL_GAP_VP : 0,
        right: this.side === 'right' ? GRIP_TOGGLE_LABEL_GAP_VP : 0
      })

      Toggle({ type: ToggleType.Switch, isOn: this.isOn })
        .enabled(this.enabled)
        .onChange((isOn: boolean) => this.onChange(isOn))
        .offset({ x: this.toggleOffsetX })
        .onAreaChange((_oldArea: Area, newArea: Area) => this.updateToggleWidth(newArea))
    }
    .width('100%')
    .alignItems(VerticalAlign.Center)
    .onAreaChange((_oldArea: Area, newArea: Area) => this.updateRowWidth(newArea))
  }
}
