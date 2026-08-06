package com.xiwei.sujian.editor.v2.input

import android.view.View
import android.view.inputmethod.InputMethodManager
import org.robolectric.annotation.Implementation
import org.robolectric.annotation.Implements
import org.robolectric.shadows.ShadowInputMethodManager

/**
 * Robolectric shadow that records every [InputMethodManager.updateSelection] invocation,
 * so the JVM contract tests can assert that [AndroidInputConnection.setComposingRegion]
 * produces no extra selection callback while commitText / setComposingText / setSelection
 * keep notifying the IME exactly once per call.
 *
 * Extends the default [ShadowInputMethodManager] so all other InputMethodManager behavior
 * is unchanged; only `updateSelection` is intercepted. The counter is class-level because
 * the InputMethodManager instance is created by the system service registry — every
 * [InputConnectionTestHarness] resets it in its init block.
 */
@Implements(InputMethodManager::class)
class RecordingInputMethodManagerShadow : ShadowInputMethodManager() {

    companion object {
        @JvmStatic
        var updateSelectionCount: Int = 0
            private set

        @JvmStatic
        fun resetUpdateSelectionCount() {
            updateSelectionCount = 0
        }
    }

    @Implementation
    @Suppress("ProtectedMemberInFinalClass") // Robolectric Shadow 要求 protected
    protected fun updateSelection(
        view: View?,
        selStart: Int,
        selEnd: Int,
        candidatesStart: Int,
        candidatesEnd: Int
    ) {
        updateSelectionCount++
    }
}
