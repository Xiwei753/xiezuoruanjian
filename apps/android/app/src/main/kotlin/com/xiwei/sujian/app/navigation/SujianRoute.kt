package com.xiwei.sujian.app.navigation

import android.os.Parcelable
import androidx.navigation3.runtime.NavKey
import kotlinx.parcelize.Parcelize
import kotlinx.serialization.Serializable

/**
 * 全局 Navigation3 back stack 只保留真正的一级 destination。
 *
 * 作品、卷、章节选择属于写作工作区内部状态（[com.xiwei.sujian.app.SujianAppState]），
 * 设置分类属于设置壳内部列表-详情状态，都不再作为全局 route 入栈。
 */
sealed interface SujianRoute : Parcelable, NavKey {
    @Parcelize
    @Serializable
    data object Works : SujianRoute

    @Parcelize
    @Serializable
    data object StarMap : SujianRoute

    @Parcelize
    @Serializable
    data object Stats : SujianRoute

    @Parcelize
    @Serializable
    data object Settings : SujianRoute

    /**
     * 全局搜索页 — 作为当前一级栈上的覆盖页，返回后回到原页面与原工作区状态。
     *
     * 语境字段只用于 Core 排序加权与结果跳转，不复制工作区业务状态。
     */
    @Parcelize
    @Serializable
    data class Search(
        val originScreen: String = "general",
        val scope: String = "all",
        val topLevel: String = "Works",
        val projectId: String? = null,
        val volumeId: String? = null,
        val chapterId: String? = null,
        val starmapId: String? = null,
        val settingsSection: String? = null,
    ) : SujianRoute
}

/**
 * 设置分类枚举。仅用于设置壳内部的列表-详情状态，不进入全局导航。
 */
@Serializable
enum class SettingsSection {
    Appearance,
    Editor,
    Save,
    Sync,
    Recovery,
    Ai,
    Diagnostics,
    Laboratory,
    About,
}
