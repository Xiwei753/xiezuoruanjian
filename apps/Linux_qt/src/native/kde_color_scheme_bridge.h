// =============================================================================
// kde_color_scheme_bridge.h — Linux_qt KDE 应用配色方案桥
// =============================================================================
//
// 把「主题模式（跟随系统 / 浅色 / 深色）」翻译成 KDE 应用配色方案切换：
//   - system → 回到系统方案（KColorSchemeManager 的空 index）
//   - dark   → BreezeDark
//   - light  → BreezeLight
//
// 只在 Linux Qt 平台适配层使用，不承载任何业务逻辑；正文配色仍由 Core 的
// theme_state_json → DesignTokens 消费，本桥只负责 KDE 壳层着色。
//
// 返回约定：0 表示已应用（含 system 复位），非 0 表示失败（未切换任何方案）。
// =============================================================================

#ifndef SUJIAN_KDE_COLOR_SCHEME_BRIDGE_H
#define SUJIAN_KDE_COLOR_SCHEME_BRIDGE_H

#ifdef __cplusplus
extern "C" {
#endif

/// 按主题模式切换 KDE 应用配色方案。
/// @param mode 必须是 "system" / "dark" / "light" 之一。
/// @return 0 成功；非 0 失败（调用方保留原有配色）。
int sujian_kde_color_scheme_apply(const char *mode);

#ifdef __cplusplus
}
#endif

#endif // SUJIAN_KDE_COLOR_SCHEME_BRIDGE_H
