package com.xiwei.sujian.core.interop.common
import android.content.Context
import com.xiwei.sujian.app.di.AppServiceProvider

/**
 * 布局策略 Repository 提供者 — UI 层通过此提供者获取 LayoutPolicyBridge。
 *
 * UI 层不应直接引用 AppServiceProvider（架构分层规则 #597），
 * 此提供者封装了 AppServiceProvider 调用。
 */
object LayoutPolicyRepositoryProvider {
    fun getLayoutPolicyBridge(context: Context): LayoutPolicyBridge {
        return AppServiceProvider.getLayoutPolicyBridge(context)
    }
}
