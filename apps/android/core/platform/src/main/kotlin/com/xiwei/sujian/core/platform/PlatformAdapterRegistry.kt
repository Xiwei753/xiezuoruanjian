package com.xiwei.sujian.core.platform

/**
 * Android 平台适配层
 *
 * 将 InputConnection / IMM / CursorAnchorInfo / ClipboardManager 等系统交互
 * 收敛到此包，SujianEditorView 只调用统一接口，不直接知道 IMM 细节。
 *
 * 实现 writer_core PlatformInteraction 定义的五大适配器：
 * - TextInputAdapter
 * - CursorAnchorAdapter
 * - AnimationDriver
 * - PlatformCapabilities
 * - ClipboardAndFocusAdapter
 */

object PlatformAdapterRegistry {
    private var _capabilities: PlatformCapabilities? = null

    val capabilities: PlatformCapabilities
        get() = _capabilities ?: PlatformCapabilities.android().also { _capabilities = it }

    fun initialize() {
        _capabilities = PlatformCapabilities.android()
    }
}
