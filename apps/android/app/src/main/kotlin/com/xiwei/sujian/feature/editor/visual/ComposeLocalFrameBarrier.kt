package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot

/**
 * #706 评论 5715257924 症状1：本地输入帧屏障 —
 * 在用户按键到权威 layout/visual scene 准备好之间，冻结上一稳定帧，
 * 防止 BasicTextField 已更新的裸正文先画一帧造成输入/删除闪烁。
 *
 * 这是一个很小的本地输入屏障状态，**不是 Compose State** —
 * 由 [ComposeEditorVisualState] 作为普通字段持有，draw 层在绘制时直接查询
 * [ComposeEditorVisualState.localFrameBarrierSnapshot]。
 *
 * - [baseScene]/[baseLayout] 表示"用户按键前屏幕真正还在显示什么"。
 *   连续快速输入时，如果上一笔 barrier 还没完成，不要换掉 [baseScene]/[baseLayout]，
 *   只更新 [expectedText]/[expectedSelection]（见 [ComposeEditorVisualState.recordLocalInput]）。
 * - [handoffPatchId] 由 [ComposeEditorVisualState.onAuthoritativeLayout] 配对出最终 local patch 后绑定，
 *   [ComposeEditorVisualState.drainPendingPatchesAtFrame] 处理到该 patch 时用 [baseScene] 作为
 *   timeline redirect 的屏幕起点，sample 出新 scene 后再清 barrier。
 *
 * @param epoch 单调递增的序号，区分连续多笔 barrier。
 * @param baseScene 用户按键前屏幕真正还在显示的视觉场景。
 * @param baseLayout 用户按键前屏幕真正还在显示的布局快照（可能为 null，首帧）。
 * @param expectedText 本笔输入预期的新正文（连续输入时只更新此字段）。
 * @param expectedSelection 本笔输入预期的新选区（连续输入时只更新此字段）。
 * @param handoffPatchId 配对出的 local patch id；null 表示尚未配对。
 */
internal data class ComposeLocalFrameBarrier(
    val epoch: Long,
    val baseScene: ComposeVisualScene,
    val baseLayout: ComposeLayoutSnapshot?,
    val expectedText: String,
    val expectedSelection: TextRange,
    val handoffPatchId: Long? = null,
)
