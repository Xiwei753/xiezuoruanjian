package com.xiwei.sujian.feature.editor.visual

interface AnimationTimeSource {
    /**
     * 此刻的 monotonic time — 生产实现直接取 System.nanoTime()，
     * 不返回缓存帧。用于事务提交时间等"现在"语义。
     */
    fun nowNanos(): Long

    /**
     * #637 评论 5386066978 项4：最近一次真实 VSync 帧的 **animation time**，
     * 只给当前屏幕帧、rebase、pause/resume 使用。
     *
     * 语义收紧：这必须是本帧真正用于渲染的 animation timestamp（API 33+ 从
     * `Choreographer.FrameData.getFrameTimeNanos()` 读取，API 30–32 从
     * `FrameCallback.doFrame` 读取）。rebase、pause/resume、draw 全部消费
     * 同一套时间基准，不混用 frame time 和 wall clock。
     *
     * 未收到过帧时返回 null。
     */
    fun lastFrameTimeNanos(): Long?
}

class ChoreographerAnimationTimeSource : AnimationTimeSource {
    /**
     * #637 评论 5386066978 项4：缓存字段语义收紧为"本帧 animation time"。
     * 由 [onFrameTimeNanos] 在每个 VSync 帧更新，rebase/pause/resume/draw 统一消费。
     */
    @Volatile
    private var lastAnimationFrameTimeNanos: Long = Long.MIN_VALUE

    /**
     * 收到本帧用于渲染的 animation timestamp（来自 VsyncCallback.FrameData 或
     * FrameCallback.doFrame）。不得传入 wall clock 或其他时间源。
     */
    fun onFrameTimeNanos(frameTimeNanos: Long) {
        lastAnimationFrameTimeNanos = frameTimeNanos
    }

    override fun nowNanos(): Long = System.nanoTime()

    override fun lastFrameTimeNanos(): Long? {
        val cached = lastAnimationFrameTimeNanos
        return if (cached != Long.MIN_VALUE) cached else null
    }
}

class TransactionIdSource {
    private var nextId: Long = 1L

    fun nextId(): Long {
        val id = nextId
        nextId = nextId.inc()
        return id
    }
}
