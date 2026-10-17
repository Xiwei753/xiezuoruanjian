// LayoutTokens — 跨页面共享的布局常量。
// Issue #778 评论5853666955 第3项：Home / Workspace 两页的 FAB 尺寸和底部间距
// 必须引用同一个常量，不要在各页面硬编码重复值。

export const PRIMARY_FAB_SIZE_VP: number = 56
export const PRIMARY_FAB_BOTTOM_INSET_VP: number = 112

// 宽屏作品卡的最小宽度（vp）。原由 Core LayoutMetrics.projectCardMinWidthDp 提供，
// Issue #920 D 后布局尺寸归平台所有，这里作为平台侧的单一来源。
export const PROJECT_CARD_MIN_WIDTH_VP: number = 180
