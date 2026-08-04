package com.xiwei.sujian.testime

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Bundle
import com.xiwei.sujian.test.testime.TestInputMethodService
import com.xiwei.sujian.test.testime.TestImeCommandService

/**
 * Deterministic command channel for the instrumented IME tests (issue #589).
 *
 * The test runs in the target app process (com.xiwei.sujian) while the test
 * IME runs in the test APK process (com.xiwei.sujian.test). Commands cross
 * that process boundary as explicit startService Intents to
 * [TestImeCommandService] (the IME service itself cannot be started by the
 * app process — it must be BIND_INPUT_METHOD-protected to be recognized as an
 * input method, and AMS rejects startService from non-system callers for
 * protected services). The relay forwards the command to the bound
 * [TestInputMethodService], which executes it against its current
 * InputConnection. Every command is logged by the IME under tag
 * "SujianTestIme".
 *
 * Offsets follow the Android InputConnection API conventions:
 * - [commitText]/[setComposingText] take a cursor position in UTF-16 code
 *   units (positive = after the committed/replacement text, 1-indexed).
 * - [setComposingRegion]/[setSelection] take UTF-16 offsets.
 */
object TestImeCommands {

    const val IME_COMPONENT = TestInputMethodService.COMPONENT_ID

    private val imeComponentName = ComponentName.unflattenFromString(TestImeCommandService.COMPONENT_ID)
        ?: error("Invalid IME command component id: ${TestImeCommandService.COMPONENT_ID}")

    fun commitText(context: Context, text: String, cursor: Int = 1) {
        send(
            context,
            TestInputMethodService.COMMAND_COMMIT_TEXT,
            Bundle().apply {
                putString(TestInputMethodService.EXTRA_TEXT, text)
                putInt(TestInputMethodService.EXTRA_CURSOR, cursor)
            }
        )
    }

    fun setComposingText(context: Context, text: String, cursor: Int = 1) {
        send(
            context,
            TestInputMethodService.COMMAND_SET_COMPOSING_TEXT,
            Bundle().apply {
                putString(TestInputMethodService.EXTRA_TEXT, text)
                putInt(TestInputMethodService.EXTRA_CURSOR, cursor)
            }
        )
    }

    fun setComposingRegion(context: Context, startUtf16: Int, endUtf16: Int) {
        send(
            context,
            TestInputMethodService.COMMAND_SET_COMPOSING_REGION,
            Bundle().apply {
                putInt(TestInputMethodService.EXTRA_START, startUtf16)
                putInt(TestInputMethodService.EXTRA_END, endUtf16)
            }
        )
    }

    fun finishComposingText(context: Context) {
        send(context, TestInputMethodService.COMMAND_FINISH_COMPOSING_TEXT)
    }

    fun setSelection(context: Context, startUtf16: Int, endUtf16: Int) {
        send(
            context,
            TestInputMethodService.COMMAND_SET_SELECTION,
            Bundle().apply {
                putInt(TestInputMethodService.EXTRA_START, startUtf16)
                putInt(TestInputMethodService.EXTRA_END, endUtf16)
            }
        )
    }

    private fun send(context: Context, command: String, extras: Bundle = Bundle()) {
        val intent = Intent().setComponent(imeComponentName)
        intent.putExtra(TestInputMethodService.EXTRA_COMMAND, command)
        intent.putExtras(extras)
        context.startService(intent)
    }
}
