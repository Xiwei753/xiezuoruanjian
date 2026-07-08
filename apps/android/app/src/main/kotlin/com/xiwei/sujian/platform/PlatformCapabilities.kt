package com.xiwei.sujian.platform

/**
 * 平台能力集合 — 启动时由平台适配层一次性报告
 *
 * 设置页和前端按钮按 capabilities 显示/禁用。
 * 鸿蒙未实现动画就禁用动画，Android 不支持 replacement commit 就不暴露。
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
        fun android(): PlatformCapabilities = PlatformCapabilities(
            supportsImePreedit = true,
            supportsCursorAnchor = true,
            supportsReplacementCommit = false,
            supportsTextAnimation = true,
            supportsSmoothCursor = true,
            supportsReflowAnimation = true,
            supportsClipboard = true,
            supportsContextMenu = true,
        )

        /** Harmony — 动画未实现，能力受限 */
        fun harmony(): PlatformCapabilities = PlatformCapabilities(
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

    fun hasAnyAnimationSupport(): Boolean =
        supportsTextAnimation || supportsSmoothCursor || supportsReflowAnimation

    fun hasAnyImeSupport(): Boolean =
        supportsImePreedit || supportsCursorAnchor || supportsReplacementCommit
}
