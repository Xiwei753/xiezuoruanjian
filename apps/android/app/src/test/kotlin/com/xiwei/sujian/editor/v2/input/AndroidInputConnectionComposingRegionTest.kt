package com.xiwei.sujian.editor.v2.input

import android.content.Context
import android.view.View
import android.view.inputmethod.InputMethodInfo
import android.view.inputmethod.InputMethodManager
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.pipeline.InputCommandPort
import com.xiwei.sujian.editor.v2.pipeline.PipelineOutput
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.CompositionSessionDto
import uniffi.writer_core.CoordinatedCursorDto
import uniffi.writer_core.EditorByteRangeDto
import uniffi.writer_core.EditorEditOutcomeDto
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.EditorVisualIntentDto

/**
 * Contract test for [AndroidInputConnection.setComposingRegion].
 *
 * setComposingRegion is only legitimate when an IME is actually enabled: the
 * InputMethodManagerService mirrors the current selection as a composing region
 * through RemoteInputConnectionImpl whenever no IME is enabled (observed on
 * emulators with all IMEs disabled — every updateSelection() from commitText
 * triggers a spurious call). Accepting those calls would mark the adapter as
 * composing and corrupt subsequent plain commits (text loss, wrong
 * operationKind), so the connection must ignore them while still honouring
 * genuine IME calls.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AndroidInputConnectionComposingRegionTest {

    private var beginCompositionCalls = 0

    private fun buildInputConnection(): Pair<AndroidInputAdapter, AndroidInputConnection> {
        beginCompositionCalls = 0
        val mirror = DisplayTextMirror()
        mirror.loadText("ABXY", 4)
        val commandPort = FakeInputCommandPort(mirror) { beginCompositionCalls++ }
        val adapter = AndroidInputAdapter(mirror, commandPort, null)
        val hostView = View(ApplicationProvider.getApplicationContext())
        adapter.setHostView(hostView)
        val connection = AndroidInputConnection(adapter, mirror, commandPort, hostView, null)
        return adapter to connection
    }

    @Test
    fun setComposingRegion_withNoEnabledIme_isIgnored() {
        // Robolectric default: no enabled IME on the device.
        val (adapter, connection) = buildInputConnection()

        val result = connection.setComposingRegion(0, 2)

        assertTrue("setComposingRegion must report success even when ignored", result)
        assertFalse(
            "With no enabled IME the spurious mirror call must not enter composing mode",
            adapter.isComposing()
        )
        assertEquals("No kernel composition may be started", 0, beginCompositionCalls)
    }

    @Test
    fun setComposingRegion_withEnabledIme_startsComposition() {
        val (adapter, connection) = buildInputConnection()
        val context = ApplicationProvider.getApplicationContext<Context>()
        val imm = context.getSystemService(InputMethodManager::class.java)
        assertNotNull("Robolectric must provide an InputMethodManager", imm)
        shadowOf(imm).setEnabledInputMethodInfoList(
            listOf(InputMethodInfo("com.example.ime", "com.example.ime.Ime", "Test IME", ""))
        )

        val result = connection.setComposingRegion(0, 2)

        assertTrue(result)
        assertTrue("With an enabled IME the call must enter composing mode", adapter.isComposing())
        assertEquals("Kernel composition must be started", 1, beginCompositionCalls)
        assertEquals("Preedit must mirror the composing region text", "AB", adapter.getCompositionText())
    }

    private class FakeInputCommandPort(
        override val mirror: DisplayTextMirror,
        private val onBeginComposition: () -> Unit
    ) : InputCommandPort {
        override fun insertText(byteOffset: Int, text: String, cause: EditorTransactionCauseDto): PipelineOutput =
            PipelineOutput.NeedReload

        override fun deleteRange(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto): PipelineOutput =
            PipelineOutput.NeedReload

        override fun replaceRangeTyped(
            byteStart: Int,
            byteEndExclusive: Int,
            replacementText: String,
            originalText: String,
            cause: EditorTransactionCauseDto,
            beforePatch: (() -> Unit)?
        ): PipelineOutput = PipelineOutput.NeedReload

        override fun setSelectionTyped(anchorByteOffset: Int, headByteOffset: Int): PipelineOutput =
            PipelineOutput.NeedReload

        override fun applyEditResult(result: com.xiwei.sujian.editor.v2.mirror.EditResult, beforePatch: (() -> Unit)?): PipelineOutput =
            PipelineOutput.NeedReload

        override fun applyCompositionCommit(
            dto: EditorEditResultDto,
            preeditText: String
        ): PipelineOutput = PipelineOutput.NeedReload

        override fun applyCompositionUpdateAnimated(
            replaceStartUtf8: Int,
            replaceEndUtf8: Int,
            newPreeditText: String,
            oldPreeditText: String,
            mirrorUpdate: (() -> Unit)?
        ) = Unit

        override fun applyCompositionCancelAnimated(
            replaceStartUtf8: Int,
            replaceEndUtf8: Int,
            oldPreeditText: String,
            mirrorUpdate: (() -> Unit)?
        ) = Unit

        override fun onCompositionUpdated() = Unit

        override fun reloadFromKernel(): Boolean = false

        override fun getCursorUtf8(): Int = 0

        override fun getRevision(): Long = 0

        override fun getText(): String = mirror.getText()

        override fun commitComposition(
            byteStart: Int,
            byteEndExclusive: Int,
            replacementText: String,
            resultingSelectionAnchor: Int,
            resultingSelectionHead: Int,
            compositionSessionId: Long,
            compositionBaseRevision: Long,
            compositionGeneration: Long,
            cause: EditorTransactionCauseDto
        ): EditorEditResultDto? = null

        override fun deleteSurrounding(
            beforeByteStart: Int,
            beforeByteEndExclusive: Int,
            afterByteStart: Int,
            afterByteEndExclusive: Int,
            cause: EditorTransactionCauseDto
        ): EditorEditResultDto? = null

        override fun beginComposition(replaceStart: Int, replaceEndExclusive: Int): EditorEditResultDto? {
            onBeginComposition()
            return EditorEditResultDto(
                outcome = EditorEditOutcomeDto.APPLIED,
                transactionId = 1u,
                baseRevision = 0u,
                newRevision = 1u,
                displayPatches = emptyList(),
                oldSelectionStart = replaceStart.toUInt(),
                oldSelectionEnd = replaceEndExclusive.toUInt(),
                newSelectionStart = replaceStart.toUInt(),
                newSelectionEnd = replaceEndExclusive.toUInt(),
                visualIntent = EditorVisualIntentDto(
                    cause = EditorTransactionCauseDto.TYPING,
                    operationKind = EditorOperationKindDto.COMPOSITION_UPDATE,
                    oldAffectedByteRanges = listOf(EditorByteRangeDto(replaceStart.toUInt(), replaceEndExclusive.toUInt())),
                    newAffectedByteRanges = emptyList(),
                    animationMode = AnimationModeDto.GLYPH_ANIMATION,
                    durationMs = 200u,
                    coordinatedCursor = CoordinatedCursorDto(oldByteOffset = 0u, newByteOffset = 0u, shouldAnimate = false)
                ),
                compositionSession = CompositionSessionDto(sessionId = 7u, baseRevision = 0u, generation = 0u)
            )
        }

        override fun updateComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            newPreeditText: String,
            newPreeditCursorOffset: Int
        ): EditorEditResultDto? = null

        override fun finishComposition(
            compositionSessionId: Long,
            compositionGeneration: Long
        ): EditorEditResultDto? = null

        override fun cancelComposition(
            compositionSessionId: Long,
            compositionGeneration: Long
        ): EditorEditResultDto? = null
    }
}
