package com.xiwei.sujian.feature.editor.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

/** 全局搜索结果在正文中的待定位命中（UTF-8 byte offset）。 */
data class EditorSearchReveal(
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
    val byteStart: Int,
    val byteEnd: Int,
)

/**
 * 跨导航的一次性正文定位请求 — 搜索结果页写入，正文编辑窗格消费。
 *
 * 只保存平台显示侧的选区/滚动意图，不保存正文或业务状态。
 */
object EditorSearchRevealGate {
    var pending by mutableStateOf<EditorSearchReveal?>(null)
        private set

    fun request(reveal: EditorSearchReveal) {
        pending = reveal
    }

    fun consumeFor(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): EditorSearchReveal? {
        val reveal = pending ?: return null
        if (reveal.projectId != projectId ||
            reveal.volumeId != volumeId ||
            reveal.chapterId != chapterId
        ) {
            return null
        }
        pending = null
        return reveal
    }
}
