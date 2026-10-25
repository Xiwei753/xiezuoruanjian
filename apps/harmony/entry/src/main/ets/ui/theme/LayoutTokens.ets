// LayoutTokens — 跨页面共享的布局常量。
// Issue #778 评论5853666955 第3项：Home / Workspace 两页的 FAB 尺寸和底部间距
// 必须引用同一个常量，不要在各页面硬编码重复值。
//
// Issue #932：窄屏整套界面统一（FAB / 对话框 / 卡片 / 列表间距）的公共度量
// 集中在此，页面不再各自写 420、20、16、8 等散落数值。

export const PRIMARY_FAB_SIZE_VP: number = 56
export const PRIMARY_FAB_BOTTOM_INSET_VP: number = 112

// 宽屏作品卡的最小宽度（vp）。原由 Core LayoutMetrics.projectCardMinWidthDp 提供，
// Issue #920 D 后布局尺寸归平台所有，这里作为平台侧的单一来源。
export const PROJECT_CARD_MIN_WIDTH_VP: number = 180

// ── Issue #932：窄屏对话框统一度量 ──
// 对话框内容最大宽度（vp），窄屏按屏宽收缩、宽屏停在 420。
export const COMPACT_DIALOG_MAX_WIDTH_VP: number = 420
// 对话框距屏幕左右边的安全内边距（vp）。
export const COMPACT_DIALOG_SIDE_INSET_VP: number = 24
// 对话框卡片内部四周内边距（vp）。
export const COMPACT_DIALOG_CONTENT_PADDING_VP: number = 20
// 对话框输入框高度（vp）。
export const COMPACT_TEXT_INPUT_HEIGHT_VP: number = 40

// ── Issue #932：窄屏列表卡片统一度量 ──
// 卡片之间的纵向间距（vp）。
export const COMPACT_CARD_SPACING_VP: number = 10
// 卡片内部四周内边距（vp）。
export const COMPACT_CARD_PADDING_VP: number = 16
// 卡片标题字号（vp）。
export const COMPACT_CARD_TITLE_FONT_VP: number = 17
// 卡片辅助字号（vp）。
export const COMPACT_CARD_SUPPORTING_FONT_VP: number = 13
// HDS 一级页签栏可见高度、悬浮位置与内容之间的间距，共同确定滚动列表的底部避让量。
// 不叠加 safeBottomVp：系统安全区与应用绘制的悬浮栏分属不同空间。
export const FLOATING_BAR_HEIGHT_VP: number = 56
export const FLOATING_BAR_BOTTOM_MARGIN_VP: number = 28
export const FLOATING_BAR_CONTENT_GAP_VP: number = 16
export const FLOATING_BAR_CONTENT_BOTTOM_INSET_VP: number =
  FLOATING_BAR_HEIGHT_VP + FLOATING_BAR_BOTTOM_MARGIN_VP + FLOATING_BAR_CONTENT_GAP_VP

// 现有窄屏列表共用同一份一级悬浮导航避让量。
export const COMPACT_LIST_BOTTOM_INSET_VP: number = FLOATING_BAR_CONTENT_BOTTOM_INSET_VP
