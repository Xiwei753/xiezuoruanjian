package com.xiwei.sujian.editor.v2.projection

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Rect
import android.graphics.RectF
import android.text.TextPaint
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision
import com.xiwei.sujian.editor.v2.layout.AndroidLineSnapshot
import com.xiwei.sujian.editor.v2.layout.LineClusterSnapshot
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutRevision.LineRange
import com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.pipeline.AndroidVisualRuntime
import com.xiwei.sujian.editor.v2.visual.AndroidTextAnimationEngine
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class TargetDisplayRuntimeCompleteAfterDrawTest {

    private companion object {
        const val EDITOR_WIDTH = 720
        const val EDITOR_HEIGHT = 1280
        const val LINE_HEIGHT = 48
        const val DURATION_MS = 200L
    }

    private fun makeLayoutRevision(
        lineCount: Int,
        text: String = "Hello world"
    ): AndroidLayoutRevision {
        val lineRanges = mutableListOf<LineRange>()
        var byteOffset = 0
        for (i in 0 until lineCount) {
            val lineText = if (i == 0) text else "line $i"
            val lineBytes = lineText.toByteArray(Charsets.UTF_8).size
            lineRanges.add(LineRange(
                startUtf8 = byteOffset,
                endUtf8 = byteOffset + lineBytes,
                startUtf16 = byteOffset,
                endUtf16 = byteOffset + lineBytes,
                top = i * LINE_HEIGHT.toFloat(),
                bottom = (i + 1) * LINE_HEIGHT.toFloat(),
                left = 0f,
                right = EDITOR_WIDTH.toFloat(),
                paragraphId = 0,
                baseline = (i + 1) * LINE_HEIGHT.toFloat() - 10f
            ))
            byteOffset += lineBytes
        }
        return AndroidLayoutRevision(
            revisionId = 1L,
            editorRevision = 1L,
            widthFingerprint = EDITOR_WIDTH.toFloat(),
            fontFingerprint = "test",
            lineCount = lineRanges.size,
            lineRanges = lineRanges,
            cursorUtf8 = byteOffset,
            cursorUtf16 = text.length,
            cursorX = byteOffset.toFloat(),
            cursorY = 0f,
            cursorHeight = LINE_HEIGHT.toFloat(),
            selectionAnchorUtf8 = -1,
            selectionHeadUtf8 = -1,
            selectionAnchorUtf16 = -1,
            selectionHeadUtf16 = -1,
            compositionStartUtf16 = -1,
            compositionEndUtf16 = -1,
            snapshotHandles = emptyList()
        )
    }

    private fun makeSnapshot(
        id: Long,
        lineIndex: Int,
        byteStart: Int,
        byteEnd: Int
    ): AndroidLineSnapshot {
        val bitmap = Bitmap.createBitmap(EDITOR_WIDTH, LINE_HEIGHT, Bitmap.Config.ARGB_8888)
        return AndroidLineSnapshot(
            snapshotId = id,
            bitmap = bitmap,
            lineIndex = lineIndex,
            sourceRect = Rect(0, 0, EDITOR_WIDTH, LINE_HEIGHT),
            destinationRect = RectF(0f, lineIndex * LINE_HEIGHT.toFloat(), EDITOR_WIDTH.toFloat(), (lineIndex + 1) * LINE_HEIGHT.toFloat()),
            documentByteStart = byteStart,
            documentByteEndExclusive = byteEnd,
            clusters = listOf(LineClusterSnapshot(
                clusterId = 0,
                documentByteStart = byteStart,
                documentByteEndExclusive = byteEnd,
                documentUtf16Start = byteStart,
                documentUtf16EndExclusive = byteEnd,
                sourceRectInLineImage = Rect(0, 0, EDITOR_WIDTH, LINE_HEIGHT),
                visualRectInDocument = RectF(0f, lineIndex * LINE_HEIGHT.toFloat(), EDITOR_WIDTH.toFloat(), (lineIndex + 1) * LINE_HEIGHT.toFloat()),
                shapingFingerprint = "default",
                shapingIdentityConfident = true
            ))
        )
    }

    private fun makeInsertVisualIntent(): VisualIntent = VisualIntent(
        cause = uniffi.writer_core.EditorTransactionCauseDto.TYPING,
        operationKind = uniffi.writer_core.EditorOperationKindDto.INSERT,
        oldAffectedByteRanges = emptyList(),
        newAffectedByteRanges = listOf(Pair(5, 15)),
        animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
        durationMs = DURATION_MS,
        coordinatedCursor = CoordinatedCursor(0, 0, true)
    )

    private fun getVisualRuntime(runtime: TargetDisplayRuntime): AndroidVisualRuntime {
        val field = TargetDisplayRuntime::class.java.getDeclaredField("visualRuntime")
        field.isAccessible = true
        return field.get(runtime) as AndroidVisualRuntime
    }

    private fun createRuntime(
        manualTimeSource: ManualAnimationTimeSource,
        transactionIdSource: TransactionIdSource
    ): TargetDisplayRuntime {
        val mirror = DisplayTextMirror()
        mirror.loadText("Hello world", 11)
        val paint = TextPaint()
        paint.textSize = 40f
        val runtime = TargetDisplayRuntime(mirror, paint, manualTimeSource, transactionIdSource)
        runtime.setWidth(EDITOR_WIDTH.toFloat())
        runtime.setViewportSize(EDITOR_WIDTH, EDITOR_HEIGHT)
        return runtime
    }

    private fun submitInsertAnimation(visualRuntime: AndroidVisualRuntime) {
        val engineField = AndroidVisualRuntime::class.java.getDeclaredField("animationEngine")
        engineField.isAccessible = true
        val engine = engineField.get(visualRuntime) as AndroidTextAnimationEngine

        val oldRev = makeLayoutRevision(1, "Hello world")
        val newRev = makeLayoutRevision(1, "Hello beautiful world")
        val visualIntent = makeInsertVisualIntent()

        val newSnapshot = makeSnapshot(1L, 0, 5, 15)
        val prepared = engine.prepare(
            visualIntent,
            oldRevision = oldRev,
            newRevision = newRev,
            newSnapshots = mapOf(0 to newSnapshot)
        )
        engine.submit(prepared)
    }

    @Test
    fun drawFrame_completesTransactionWhenCompleteAfterDrawIsTrue() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = createRuntime(manualTimeSource, transactionIdSource)
        val visualRuntime = getVisualRuntime(runtime)

        manualTimeSource.advanceByMs(1)
        submitInsertAnimation(visualRuntime)

        assertTrue(
            "After submit, should have active animation",
            runtime.hasActiveAnimation()
        )

        val startFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(startFrameNanos)

        manualTimeSource.advanceByMs(DURATION_MS + 1)
        val endFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(endFrameNanos)

        assertTrue(
            "After onFrame at 100%, transaction should still be active (not yet drawn)",
            runtime.hasActiveAnimation()
        )

        val bitmap = Bitmap.createBitmap(EDITOR_WIDTH, EDITOR_HEIGHT, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        runtime.drawFrame(canvas)

        assertFalse(
            "After drawFrame with completeAfterDraw=true, animation should be completed",
            runtime.hasActiveAnimation()
        )
    }

    @Test
    fun drawFrame_needsFrameBecomesFalseAfterCompletion() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = createRuntime(manualTimeSource, transactionIdSource)
        val visualRuntime = getVisualRuntime(runtime)

        manualTimeSource.advanceByMs(1)
        submitInsertAnimation(visualRuntime)

        val startFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(startFrameNanos)

        manualTimeSource.advanceByMs(DURATION_MS + 1)
        val endFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(endFrameNanos)

        val bitmap = Bitmap.createBitmap(EDITOR_WIDTH, EDITOR_HEIGHT, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        runtime.drawFrame(canvas)

        assertFalse(
            "After drawFrame completes transaction, needsFrame should be false",
            runtime.needsFrame()
        )
    }

    @Test
    fun onFrame_preservesCompleteAfterDrawInCachedState() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = createRuntime(manualTimeSource, transactionIdSource)
        val visualRuntime = getVisualRuntime(runtime)

        manualTimeSource.advanceByMs(1)
        submitInsertAnimation(visualRuntime)

        val startFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(startFrameNanos)

        manualTimeSource.advanceByMs(DURATION_MS + 1)
        val endFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(endFrameNanos)

        assertTrue(
            "After onFrame at 100%, transaction must still be active (completeAfterDraw not yet consumed by drawFrame)",
            runtime.hasActiveAnimation()
        )

        val bitmap = Bitmap.createBitmap(EDITOR_WIDTH, EDITOR_HEIGHT, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        runtime.drawFrame(canvas)

        assertFalse(
            "drawFrame must complete the transaction when completeAfterDraw was true in cached FrameState",
            runtime.hasActiveAnimation()
        )
    }

    @Test
    fun drawFrame_doesNotCompleteTransactionBeforeAnimationEnd() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = createRuntime(manualTimeSource, transactionIdSource)
        val visualRuntime = getVisualRuntime(runtime)

        manualTimeSource.advanceByMs(1)
        submitInsertAnimation(visualRuntime)

        val startFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(startFrameNanos)

        manualTimeSource.advanceByMs(50)
        val midFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(midFrameNanos)

        val bitmap = Bitmap.createBitmap(EDITOR_WIDTH, EDITOR_HEIGHT, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        runtime.drawFrame(canvas)

        assertTrue(
            "After drawFrame at 50%, animation should still be active (completeAfterDraw=false)",
            runtime.hasActiveAnimation()
        )
    }

    @Test
    fun drawFrame_fallbackPathCompletesTransactionWhenCompleteAfterDrawIsTrue() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = createRuntime(manualTimeSource, transactionIdSource)
        val visualRuntime = getVisualRuntime(runtime)

        manualTimeSource.advanceByMs(1)
        submitInsertAnimation(visualRuntime)

        val startFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(startFrameNanos)

        manualTimeSource.advanceByMs(DURATION_MS + 1)
        val endFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(endFrameNanos)

        runtime.invalidateDisplayState()

        val bitmap = Bitmap.createBitmap(EDITOR_WIDTH, EDITOR_HEIGHT, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        runtime.drawFrame(canvas)

        assertFalse(
            "drawFrame fallback path must complete transaction when completeAfterDraw=true",
            runtime.hasActiveAnimation()
        )

        assertFalse(
            "After fallback drawFrame completes transaction, needsFrame should be false",
            runtime.needsFrame()
        )
    }

    @Test
    fun drawFrame_completesTransactionInMaskedProjection() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = createRuntime(manualTimeSource, transactionIdSource)
        val visualRuntime = getVisualRuntime(runtime)

        runtime.setSecretMasked(true)

        manualTimeSource.advanceByMs(1)
        submitInsertAnimation(visualRuntime)

        assertTrue(
            "After submit in masked projection, should have active animation",
            runtime.hasActiveAnimation()
        )

        val startFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(startFrameNanos)

        manualTimeSource.advanceByMs(DURATION_MS + 1)
        val endFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(endFrameNanos)

        assertTrue(
            "After onFrame at 100% in masked projection, transaction must still be active",
            runtime.hasActiveAnimation()
        )

        val bitmap = Bitmap.createBitmap(EDITOR_WIDTH, EDITOR_HEIGHT, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        runtime.drawFrame(canvas)

        assertFalse(
            "After drawFrame in masked projection, animation should be completed",
            runtime.hasActiveAnimation()
        )

        assertFalse(
            "After drawFrame completes transaction in masked projection, needsFrame should be false",
            runtime.needsFrame()
        )
    }

    @Test
    fun drawFrame_completesRebasedTransactionWhenCompleteAfterDrawIsTrue() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = createRuntime(manualTimeSource, transactionIdSource)
        val visualRuntime = getVisualRuntime(runtime)

        manualTimeSource.advanceByMs(1)
        submitInsertAnimation(visualRuntime)

        val startFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(startFrameNanos)

        manualTimeSource.advanceByMs(50)
        val midFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(midFrameNanos)

        assertTrue(
            "After onFrame at 50%, first animation should still be active",
            runtime.hasActiveAnimation()
        )

        submitInsertAnimation(visualRuntime)

        assertTrue(
            "After rebase submit, should have active animation (new transaction)",
            runtime.hasActiveAnimation()
        )

        val rebaseFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(rebaseFrameNanos)

        manualTimeSource.advanceByMs(DURATION_MS + 1)
        val endFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(endFrameNanos)

        assertTrue(
            "After onFrame at 100% of rebased animation, transaction must still be active",
            runtime.hasActiveAnimation()
        )

        val bitmap = Bitmap.createBitmap(EDITOR_WIDTH, EDITOR_HEIGHT, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        runtime.drawFrame(canvas)

        assertFalse(
            "After drawFrame completes rebased transaction, animation should be completed",
            runtime.hasActiveAnimation()
        )

        assertFalse(
            "After drawFrame completes rebased transaction, needsFrame should be false",
            runtime.needsFrame()
        )
    }

    @Test
    fun drawFrame_maskedProjectionFallbackPathCompletesTransaction() {
        val manualTimeSource = ManualAnimationTimeSource()
        val transactionIdSource = TransactionIdSource()
        val runtime = createRuntime(manualTimeSource, transactionIdSource)
        val visualRuntime = getVisualRuntime(runtime)

        runtime.setSecretMasked(true)

        manualTimeSource.advanceByMs(1)
        submitInsertAnimation(visualRuntime)

        val startFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(startFrameNanos)

        manualTimeSource.advanceByMs(DURATION_MS + 1)
        val endFrameNanos = manualTimeSource.nowNanos()
        runtime.onFrame(endFrameNanos)

        runtime.invalidateDisplayState()

        val bitmap = Bitmap.createBitmap(EDITOR_WIDTH, EDITOR_HEIGHT, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        runtime.drawFrame(canvas)

        assertFalse(
            "drawFrame fallback path in masked projection must complete transaction",
            runtime.hasActiveAnimation()
        )

        assertFalse(
            "After fallback drawFrame in masked projection, needsFrame should be false",
            runtime.needsFrame()
        )
    }
}
