// GripAdaptiveToggleRow — 设置行中的握持侧自适应开关。
// 保持单个 Toggle 实例，用同一数值进度同步移动开关并为标签避让。

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

  @BuilderParam textBuilder: () => void = this.emptyText

  @Local private rowWidth: number = 0
  @Local private toggleWidth: number = 0
  @Local private sideProgress: number = 0
  private layoutInitialized: boolean = false

  private initializeSideProgress(): void {
    if (this.layoutInitialized || this.rowWidth <= 0 || this.toggleWidth <= 0) {
      return
    }
    this.sideProgress = this.side === 'left' ? 1 : 0
    this.layoutInitialized = true
  }

  private updateRowWidth(area: Area): void {
    const measuredWidth: number = Number(area.width)
    if (measuredWidth <= 0 || measuredWidth === this.rowWidth) {
      return
    }
    this.rowWidth = measuredWidth
    this.initializeSideProgress()
  }

  private updateToggleWidth(area: Area): void {
    const measuredWidth: number = Number(area.width)
    if (measuredWidth <= 0 || measuredWidth === this.toggleWidth) {
      return
    }
    this.toggleWidth = measuredWidth
    this.initializeSideProgress()
  }

  @Monitor('side')
  onGripSideChanged(): void {
    const targetProgress: number = this.side === 'left' ? 1 : 0
    if (!this.layoutInitialized) {
      this.sideProgress = targetProgress
      return
    }
    this.getUIContext().animateTo({
      duration: GRIP_TOGGLE_SWITCH_DURATION_MS,
      curve: Curve.EaseInOut
    }, () => {
      this.sideProgress = targetProgress
    })
  }

  private toggleOffsetX(): number {
    return -Math.max(0, this.rowWidth - this.toggleWidth) * this.sideProgress
  }

  private labelLeftPadding(): number {
    if (this.sideProgress <= 0.5) {
      return 0
    }
    return Math.max(0, this.rowWidth - this.toggleWidth) * (1 - this.sideProgress) +
      this.toggleWidth + GRIP_TOGGLE_LABEL_GAP_VP
  }

  private labelRightPadding(): number {
    if (this.sideProgress > 0.5) {
      return 0
    }
    return Math.max(0, this.rowWidth - this.toggleWidth) * this.sideProgress +
      GRIP_TOGGLE_LABEL_GAP_VP
  }

  private labelOpacity(): number {
    return Math.min(1, Math.abs(this.sideProgress - 0.5) * 10)
  }

  build() {
    Row() {
      Column({ space: 2 }) {
        this.textBuilder()
      }
      .layoutWeight(1)
      .alignItems(HorizontalAlign.Start)
      .opacity(this.labelOpacity())
      .padding({
        left: this.labelLeftPadding(),
        right: this.labelRightPadding()
      })

      Toggle({ type: ToggleType.Switch, isOn: this.isOn })
        .enabled(this.enabled)
        .onChange((isOn: boolean) => this.onChange(isOn))
        .offset({ x: this.toggleOffsetX() })
        .onAreaChange((_oldArea: Area, newArea: Area) => this.updateToggleWidth(newArea))
    }
    .width('100%')
    .alignItems(VerticalAlign.Center)
    .onAreaChange((_oldArea: Area, newArea: Area) => this.updateRowWidth(newArea))
  }
}
