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

    fun advanceToProgress(progress: Float, durationMs: Long, startTimeMs: Long = 0L) {
        require(progress in 0f..1f) { "progress must be in [0, 1]: $progress" }
        require(durationMs >= 0) { "durationMs must be non-negative: $durationMs" }
        val targetMs = startTimeMs + (durationMs * progress).toLong()
        // "Advance" must never move the clock backward: a real vsync frame may already
        // have marked the animation start and advanced the clock past the requested
        // progress point, in which case the clock stays where it is.
        advanceTo(maxOf(targetMs * 1_000_000L, currentTimeNanos))
    }

    fun advanceToEnd(durationMs: Long, startTimeMs: Long = 0L) {
        advanceToProgress(1f, durationMs, startTimeMs)
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
