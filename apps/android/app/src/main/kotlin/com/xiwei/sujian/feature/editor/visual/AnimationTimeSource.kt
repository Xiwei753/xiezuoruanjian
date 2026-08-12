package com.xiwei.sujian.feature.editor.visual

interface AnimationTimeSource {
    /**
     * 此刻的 monotonic time — 生产实现直接取 System.nanoTime()，
     * 不返回缓存帧。用于事务提交时间等"现在"语义。
     */
    fun nowNanos(): Long

    /**
     * 最近一次真实 VSync 帧时间，只给当前屏幕帧、rebase、pause/resume 使用。
     * 未收到过帧时返回 null。
     */
    fun lastFrameTimeNanos(): Long?
}

class ChoreographerAnimationTimeSource : AnimationTimeSource {
    @Volatile
    private var lastFrameTimeNanos: Long = Long.MIN_VALUE

    fun onFrameTimeNanos(frameTimeNanos: Long) {
        lastFrameTimeNanos = frameTimeNanos
    }

    override fun nowNanos(): Long = System.nanoTime()

    override fun lastFrameTimeNanos(): Long? {
        val cached = lastFrameTimeNanos
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
