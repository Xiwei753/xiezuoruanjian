package com.xiwei.sujian.core.interop.stats
import android.content.Context
import com.xiwei.sujian.app.di.AppServiceProvider

/**
 * 统计 Repository 提供者 — UI 层通过此提供者获取 StatsBridge。
 *
 * UI 层不应直接引用 AppServiceProvider（架构分层规则 #597），
 * 此提供者封装了 AppServiceProvider 调用，UI 层只引用 data 包的 Repository/Provider。
 */
object StatsRepositoryProvider {
    fun getStatsBridge(context: Context): StatsBridge {
        return AppServiceProvider.getStatsBridge(context)
    }
}
