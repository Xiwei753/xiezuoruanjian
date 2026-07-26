package com.xiwei.sujian.editor.v2.visual

interface AnimationTimeSource {
    fun nowNanos(): Long
}

class ChoreographerAnimationTimeSource : AnimationTimeSource {
    override fun nowNanos(): Long = System.nanoTime()
}

class ManualAnimationTimeSource : AnimationTimeSource {
    @Volatile
    private var currentTimeNanos: Long = 0L

    fun advanceTo(timeNanos: Long) {
        require(timeNanos >= currentTimeNanos) {
            "ManualAnimationTimeSource must advance monotonically: current=$currentTimeNanos, requested=$timeNanos"
        }
        currentTimeNanos = timeNanos
    }

    fun advanceBy(deltaNanos: Long) {
        require(deltaNanos >= 0) {
            "ManualAnimationTimeSource delta must be non-negative: $deltaNanos"
        }
        currentTimeNanos += deltaNanos
    }

    fun advanceByMs(deltaMs: Long) {
        advanceBy(deltaMs * 1_000_000L)
    }

    override fun nowNanos(): Long = currentTimeNanos
}

class TransactionIdSource {
    private var nextId: Long = 1L

    fun nextId(): Long {
        val id = nextId
        nextId = nextId.inc()
        return id
    }
}
