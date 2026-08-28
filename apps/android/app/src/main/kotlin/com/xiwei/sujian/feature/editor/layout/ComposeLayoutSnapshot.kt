package com.xiwei.sujian.feature.editor.layout

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange

/**
 * #641 评论1 第4节：排版层只认 [BasicTextField] 给出的 [TextLayoutResult]。
 *
 * [ComposeLayoutSnapshot] 是从系统 [TextLayoutResult] 读出的不可变快照，
 * 不再用 `DynamicLayout` / `StaticLayout` 重新算一次软换行。自动换行落在哪一行
 * 只有 [BasicTextField] 的 [TextLayoutResult] 一个答案。
 *
 * @param result 系统 [BasicTextField] 的 `onTextLayout` 给出的最终布局结果。
 * @param selection 当前选区（UTF-16 offset）。
 * @param scrollY 当前滚动位置（px）。
 */
data class ComposeLayoutSnapshot(
    val result: TextLayoutResult,
    val selection: TextRange,
    val scrollY: Int,
)

/**
 * #641 评论1 第5节：视觉光标矩形 — 从真实 [TextLayoutResult] 取，
 * 不再由动画层或 View 自行推算。
 */
fun ComposeLayoutSnapshot.cursorRect(): Rect = result.getCursorRect(selection.end)

/**
 * #641 评论1 第4节：行信息访问 — 直接转发 [TextLayoutResult]，
 * 不缓存第二份行段。
 */
fun ComposeLayoutSnapshot.lineForOffset(offset: Int): Int = result.getLineForOffset(offset)

fun ComposeLayoutSnapshot.boundingBox(offset: Int): Rect = result.getBoundingBox(offset)
