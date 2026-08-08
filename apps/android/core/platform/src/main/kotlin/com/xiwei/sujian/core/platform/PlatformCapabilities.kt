package com.xiwei.sujian.core.platform

/**
 * 平台能力集合 — 启动时由平台适配层一次性报告
 *
 * 能力声明必须与 core/writer_core/src/platform_interaction/capabilities.rs 的 android() 工厂方法对齐。
 * 未真实接入的能力必须为 false，不允许吹牛。
 *
 * Android 已真实接入：
 * - IME preedit: SujianInputConnection → EditorInputController
 * - cursor anchor: CursorAnchorInfo → EditorView
 * - text animation: Core visual transaction → SujianEditorView animation layer
 * - smooth cursor: SujianEditorView cursor layer
 * - reflow animation: Core reflow visual transaction → animation layer
 * - clipboard: Android ClipboardManager
 * - context menu: Android context menu
 *
 * 未真实接入：
 * - (无)
 */
data class PlatformCapabilities(
    val supportsImePreedit: Boolean = false,
    val supportsCursorAnchor: Boolean = false,
    val supportsReplacementCommit: Boolean = false,
    val supportsTextAnimation: Boolean = false,
    val supportsSmoothCursor: Boolean = false,
    val supportsReflowAnimation: Boolean = false,
    val supportsClipboard: Boolean = false,
    val supportsContextMenu: Boolean = false,
) {
    companion object {
        /** Android 完整能力 */
        fun android(): PlatformCapabilities =
            PlatformCapabilities(
                supportsImePreedit = true,
                supportsCursorAnchor = true,
                supportsReplacementCommit = true,
                supportsTextAnimation = true,
                supportsSmoothCursor = true,
                supportsReflowAnimation = true,
                supportsClipboard = true,
                supportsContextMenu = true,
            )

        /** Harmony — 动画未实现，能力受限 */
        fun harmony(): PlatformCapabilities =
            PlatformCapabilities(
                supportsImePreedit = false,
                supportsCursorAnchor = false,
                supportsReplacementCommit = false,
                supportsTextAnimation = false,
                supportsSmoothCursor = false,
                supportsReflowAnimation = false,
                supportsClipboard = true,
                supportsContextMenu = true,
            )
    }

    fun hasAnyAnimationSupport(): Boolean = supportsTextAnimation || supportsSmoothCursor || supportsReflowAnimation

    fun hasAnyImeSupport(): Boolean = supportsImePreedit || supportsCursorAnchor || supportsReplacementCommit
}
