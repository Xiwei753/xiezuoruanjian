package com.xiwei.sujian.editor.v2.visual

interface AnimationTimeSource {
    fun nowNanos(): Long
}

class ChoreographerAnimationTimeSource : AnimationTimeSource {
    @Volatile
    private var lastFrameTimeNanos: Long = Long.MIN_VALUE

    fun onFrameTimeNanos(frameTimeNanos: Long) {
        lastFrameTimeNanos = frameTimeNanos
    }

    override fun nowNanos(): Long {
        val cached = lastFrameTimeNanos
        return if (cached != Long.MIN_VALUE) cached else System.nanoTime()
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
