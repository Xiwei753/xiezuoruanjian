package com.xiwei.writerapp.ui

import android.view.Choreographer
import android.os.Build

class EditorAnimationRuntime(private val editText: WriterEditText) {

    interface Animatable {
        /**
         * Step the animation.
         * @param frameTimeNanos the time in nanoseconds when the frame started.
         * @return true if the animation has more frames to render, false if it has finished.
         */
        fun onAnimationStep(frameTimeNanos: Long): Boolean
    }

    private val choreographer = Choreographer.getInstance()
    private val animatables = java.util.concurrent.CopyOnWriteArrayList<Animatable>()
    private var isRunning = false
    private var lastFrameTimeNanos = -1L

    // For frame rate and dropped frames estimation
    private var frameCount = 0
    private var droppedFrameCount = 0
    private var lastLogTimeMs = 0L

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!isRunning) return

            // Check if page is visible or attached
            if (!editText.isAttachedToWindow || !editText.isShown) {
                isRunning = false
                lastFrameTimeNanos = -1L
                choreographer.removeFrameCallback(this)
                return
            }

            // Debug frame analysis
            frameCount++
            if (lastFrameTimeNanos != -1L) {
                val frameDeltaNs = frameTimeNanos - lastFrameTimeNanos
                val currentRefreshRate = getRefreshRate()
                val expectedFrameTimeNs = (1_000_000_000L / currentRefreshRate).toLong()
                if (frameDeltaNs > expectedFrameTimeNs * 1.5) {
                    droppedFrameCount++
                }
            }
            lastFrameTimeNanos = frameTimeNanos

            var hasMore = false
            var needsFullInvalidate = false
            for (anim in animatables) {
                if (anim.onAnimationStep(frameTimeNanos)) {
                    hasMore = true
                    if (anim is TypingOverlayRenderer) {
                        needsFullInvalidate = true
                    }
                } else {
                    animatables.remove(anim)
                }
            }

            // Request draw using postInvalidateOnAnimation() if needed
            if (needsFullInvalidate) {
                editText.postInvalidateOnAnimation()
            }

            if (debugEnabled) {
                val nowMs = System.currentTimeMillis()
                if (nowMs - lastLogTimeMs >= 2000L) {
                    val typingDuration = editText.typingAnimationDurationMs()
                    val cursorDuration = editText.cursorAnimationDurationMs()
                    val fps = (frameCount * 1000f) / (nowMs - lastLogTimeMs)
                    android.util.Log.d(
                        "WriterEditorRuntime",
                        "AnimationRuntime: running=$isRunning, typingDuration=${typingDuration}ms, cursorDuration=${cursorDuration}ms, FPS=${String.format("%.1f", fps)}, Dropped frames=${droppedFrameCount}"
                    )
                    frameCount = 0
                    droppedFrameCount = 0
                    lastLogTimeMs = nowMs
                }
            }

            if (hasMore && animatables.isNotEmpty()) {
                choreographer.postFrameCallback(this)
            } else {
                isRunning = false
                lastFrameTimeNanos = -1L
            }
        }
    }

    fun register(anim: Animatable) {
        if (!animatables.contains(anim)) {
            animatables.add(anim)
        }
        if (!isRunning) {
            isRunning = true
            lastFrameTimeNanos = -1L
            choreographer.removeFrameCallback(frameCallback) // Safely avoid duplicate posts
            choreographer.postFrameCallback(frameCallback)
            requestHighRefreshRate()
        }
    }

    fun unregister(anim: Animatable) {
        animatables.remove(anim)
        if (animatables.isEmpty()) {
            isRunning = false
            lastFrameTimeNanos = -1L
            choreographer.removeFrameCallback(frameCallback)
        }
    }

    fun clear() {
        animatables.clear()
        isRunning = false
        lastFrameTimeNanos = -1L
        choreographer.removeFrameCallback(frameCallback)
    }

    fun isRunning(): Boolean = isRunning

    private fun getRefreshRate(): Float {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            editText.context.display?.refreshRate?.let {
                if (it > 0f) return it
            }
        }
        return 60f
    }

    private fun requestHighRefreshRate() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val context = editText.context
            val display = context.display
            if (display != null) {
                var maxRefreshRate = 60f
                for (mode in display.supportedModes) {
                    maxRefreshRate = kotlin.math.max(maxRefreshRate, mode.refreshRate)
                }
                (context as? android.app.Activity)?.window?.let { window ->
                    window.attributes = window.attributes.apply {
                        preferredRefreshRate = maxRefreshRate
                    }
                }
            }
        }
    }

    companion object {
        var debugEnabled = false
    }
}
