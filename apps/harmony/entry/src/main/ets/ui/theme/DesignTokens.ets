// =============================================================================
// DesignTokens.ets — 跨端 UI token 契约（Harmony 端）
// =============================================================================
//
// 与 Android shape_appearance.xml / Desktop DesignTokens.qml 同名同值。
// 禁止组件继续硬编码圆角、阴影、颜色。
// =============================================================================

export class DesignTokens {
  // --- Radius ---
  static readonly radiusXs: number = 4
  static readonly radiusSm: number = 8
  static readonly radiusMd: number = 12
  static readonly radiusLg: number = 16
  static readonly radiusXl: number = 28
  static readonly radiusPill: number = 999

  // --- Component shape tokens ---
  static readonly cardRadius: number = DesignTokens.radiusLg
  static readonly dialogRadius: number = DesignTokens.radiusXl
  static readonly fabRadius: number = DesignTokens.radiusLg
  static readonly bottomBarRadius: number = 0
  static readonly inputFieldRadius: number = DesignTokens.radiusMd

  // --- Elevation ---
  static readonly elevation0: number = 0
  static readonly elevation1: number = 1
  static readonly elevation2: number = 3
  static readonly elevation3: number = 6

  // --- Shadow presets (radius, color, offsetY) ---
  static readonly shadowLight = { radius: 4, color: '#1A000000', offsetY: 2 }
  static readonly shadowMedium = { radius: 8, color: '#33000000', offsetY: 4 }
  static readonly shadowStrong = { radius: 12, color: '#40000000', offsetY: 6 }

  // --- Surface tokens ---
  static readonly surfaceContainerLowest: string = 'surfaceContainerLowest'
  static readonly surfaceContainerLow: string = 'surfaceContainerLow'
  static readonly surfaceContainer: string = 'surfaceContainer'
  static readonly surfaceContainerHigh: string = 'surfaceContainerHigh'
  static readonly surfaceContainerHighest: string = 'surfaceContainerHighest'
}
