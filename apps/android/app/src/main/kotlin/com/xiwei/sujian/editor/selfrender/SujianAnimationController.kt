package com.xiwei.sujian.editor.selfrender

import com.xiwei.sujian.model.EditorAnimationEventData
import com.xiwei.sujian.model.EditorAnimationKindData
import com.xiwei.sujian.diagnostics.DiagnosticsLogger

/**
 * SujianAnimationController — 自研写作区动画控制器
 *
 * 管理 EditorAnimationEvent 的接收、分发、生命周期。
 *
 * ## 动画规则
 * - 插入成功后生成 transaction，Core 返回 Insert 事件，layout 计算新 glyph rect，
 *   静态层动画期间跳过 inserted range，动画层从 oldCursorRect 吐到 glyphRect
 * - 删除：删除前记录 deletedText/deletedGlyphRects/oldCursorRect，buffer 删除，
 *   Core 返回 Delete，layout 算 newCursorRect，动画层把 deletedGlyphRects 吞向 newCursorRect
 * - 连续删除每次独立 animation id，不允许 pendingDelete 覆盖丢动画
 * - 输入类型：普通单字 Typing；中文/日文 IME commit 多字 TypingCommit；
 *   composing ImeComposition 不动画；粘贴 Paste 不动画；加载 Load 不动画；
 *   设置变化 Format/Programmatic 不动画
 * - 滚动中不播放动画
 */
class SujianAnimationController(
    private val buffer: SujianEditorBuffer,
    private val layout: SujianEditorLayout,
    private val renderer: SujianEditorRenderer
) {
    private val TAG = "SujianAnimCtrl"
    
    var animationEnabled: Boolean = false
    var animationDurationMs: Long = 160L
    
    // ── 删除前快照 ──
    // 每次删除操作独立记录，不允许 pendingDelete 覆盖丢动画
    data class DeleteSnapshot(
        val deletedText: String,
        val deletedGlyphRects: List<SujianGlyphRect>,
        val oldCursorRect: android.graphics.RectF,
        val animationId: ULong
    )
    private val deleteSnapshots = mutableListOf<DeleteSnapshot>()
    
    // 最近一次 onBeforeDelete 返回的 animationId，
    // 用于在 handleDeleteEvent 中精确匹配 Core 返回的 Delete 事件
    private var lastDeleteSnapshotId: ULong = 0u
    
    /**
     * 记录删除前快照
     */
    fun recordDeleteSnapshot(
        deletedText: String,
        deletedGlyphRects: List<SujianGlyphRect>,
        oldCursorRect: android.graphics.RectF
    ): ULong {
        val id = nextAnimationId()
        deleteSnapshots.add(DeleteSnapshot(deletedText, deletedGlyphRects, oldCursorRect, id))
        lastDeleteSnapshotId = id
        return id
    }
    
    /**
     * 获取并移除删除快照（精确匹配）
     */
    fun consumeDeleteSnapshot(id: ULong): DeleteSnapshot? {
        val idx = deleteSnapshots.indexOfFirst { it.animationId == id }
        if (idx < 0) return null
        return deleteSnapshots.removeAt(idx)
    }
    
    /**
     * 处理 Core 返回的动画事件
     */
    fun handleAnimationEvents(events: List<EditorAnimationEventData>, cause: SujianEditCause) {
        if (!animationEnabled) return
        if (!shouldAnimate(cause)) return
        
        for (event in events) {
            when (event.kind) {
                EditorAnimationKindData.Insert -> {
                    handleInsertEvent(event)
                }
                EditorAnimationKindData.Delete -> {
                    handleDeleteEvent(event)
                }
                EditorAnimationKindData.Cursor -> {
                    // Cursor 动画由 CursorController 处理
                }
            }
        }
    }
    
    /**
     * 设置滚动状态
     */
    fun setScrolling(scrolling: Boolean) {
        renderer.setScrolling(scrolling)
    }
    
    /**
     * Tick 动画（每帧调用）
     */
    fun tick() {
        renderer.tickAnimations()
    }
    
    /**
     * 是否有活跃动画
     */
    fun hasActiveAnimations(): Boolean = renderer.hasActiveAnimations()
    
    fun onDetachedFromWindow() {
        renderer.clearAnimations()
        deleteSnapshots.clear()
    }
    
    // ── 内部方法 ──
    
    private fun shouldAnimate(cause: SujianEditCause): Boolean {
        return when (cause) {
            SujianEditCause.Typing,
            SujianEditCause.Delete,
            SujianEditCause.TypingCommit -> true
            SujianEditCause.Paste,
            SujianEditCause.Load,
            SujianEditCause.Format,
            SujianEditCause.ImeComposition,
            SujianEditCause.Programmatic -> false
        }
    }
    
    private fun handleInsertEvent(event: EditorAnimationEventData) {
        val text = buffer.text
        val rangeStartUtf16 = buffer.utf8ToUtf16(event.rangeStart)
        val rangeEndUtf16 = rangeStartUtf16 + event.text.length

        // oldCursorRect: 插入前的光标位置
        // rangeStartUtf16 是新文本中插入文本的起始位置，对应插入前的光标位置
        // 因为插入点之前的文本没有改变，所以新布局中该位置的坐标等于旧布局中的坐标
        val oldCursorRect = layout.getCursorRect(text, rangeStartUtf16)

        val glyphRects = layout.getGlyphRects(text, rangeStartUtf16, rangeEndUtf16)

        // 设置静态层跳过范围，避免插入动画期间重影
        renderer.setAnimatedInsertRange(IntRange(rangeStartUtf16, rangeEndUtf16))

        renderer.addAnimation(SujianOverlayAnim(
            id = event.id,
            kind = "insert",
            text = event.text,
            startX = oldCursorRect.left,
            startY = oldCursorRect.top,
            endX = if (glyphRects.isNotEmpty()) glyphRects.first().x else oldCursorRect.left,
            endY = if (glyphRects.isNotEmpty()) glyphRects.first().y else oldCursorRect.top,
            durationMs = event.durationMs,
            startTimeMs = System.currentTimeMillis(),
            glyphRects = glyphRects
        ))
    }
    
    private fun handleDeleteEvent(event: EditorAnimationEventData) {
        val newCursorRect = layout.getCursorRect(buffer.text, buffer.selection.head)

        // 使用 lastDeleteSnapshotId 精确匹配最近的删除快照
        val snapshot = consumeDeleteSnapshot(lastDeleteSnapshotId)
        if (snapshot != null) {
            renderer.addAnimation(SujianOverlayAnim(
                id = event.id,
                kind = "delete",
                text = event.text,
                startX = snapshot.oldCursorRect.left,
                startY = snapshot.oldCursorRect.top,
                endX = newCursorRect.left,
                endY = newCursorRect.top,
                durationMs = event.durationMs,
                startTimeMs = System.currentTimeMillis(),
                glyphRects = snapshot.deletedGlyphRects
            ))
        } else {
            // 没有匹配的快照时，尝试 FIFO fallback
            val fallbackSnapshot = deleteSnapshots.firstOrNull()
            if (fallbackSnapshot != null) {
                deleteSnapshots.remove(fallbackSnapshot)
                renderer.addAnimation(SujianOverlayAnim(
                    id = event.id,
                    kind = "delete",
                    text = event.text,
                    startX = fallbackSnapshot.oldCursorRect.left,
                    startY = fallbackSnapshot.oldCursorRect.top,
                    endX = newCursorRect.left,
                    endY = newCursorRect.top,
                    durationMs = event.durationMs,
                    startTimeMs = System.currentTimeMillis(),
                    glyphRects = fallbackSnapshot.deletedGlyphRects
                ))
            } else {
                // 完全没有快照时，使用 Core 事件信息创建简化动画
                DiagnosticsLogger.d(TAG, "No delete snapshot for event ${event.id}, using fallback")
                renderer.addAnimation(SujianOverlayAnim(
                    id = event.id,
                    kind = "delete",
                    text = event.text,
                    startX = newCursorRect.left,
                    startY = newCursorRect.top,
                    endX = newCursorRect.left,
                    endY = newCursorRect.top,
                    durationMs = event.durationMs,
                    startTimeMs = System.currentTimeMillis(),
                    glyphRects = emptyList()
                ))
            }
        }
    }
    
    companion object {
        private var globalAnimationId: ULong = 1u
        
        private fun nextAnimationId(): ULong {
            val id = globalAnimationId
            globalAnimationId = globalAnimationId.inc()
            return id
        }
    }
}
