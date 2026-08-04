package com.xiwei.sujian.ui.compose.navigation

import android.os.Parcelable
import androidx.navigation3.runtime.NavKey
import kotlinx.parcelize.Parcelize
import kotlinx.serialization.Serializable

/**
 * 全局 Navigation3 back stack 只保留真正的一级 destination。
 *
 * 作品、卷、章节选择属于写作工作区内部状态（[com.xiwei.sujian.ui.compose.SujianAppState]），
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
    Ai,
    Diagnostics,
    Laboratory,
    About,
}
