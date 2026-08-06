package com.xiwei.sujian.data

import android.content.Context

/**
 * 布局策略 Repository 提供者 — UI 层通过此提供者获取 LayoutPolicyBridge。
 *
 * UI 层不应直接引用 BridgeProvider（架构分层规则 #597），
 * 此提供者封装了 BridgeProvider 调用。
 */
object LayoutPolicyRepositoryProvider {
    fun getLayoutPolicyBridge(context: Context): LayoutPolicyBridge {
        return BridgeProvider.getLayoutPolicyBridge(context)
    }
}
