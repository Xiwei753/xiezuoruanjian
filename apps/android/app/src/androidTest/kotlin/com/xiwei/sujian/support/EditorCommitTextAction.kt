package com.xiwei.sujian.support

import android.view.View
import android.view.inputmethod.EditorInfo
import androidx.test.espresso.UiController
import androidx.test.espresso.ViewAction
import androidx.test.espresso.matcher.ViewMatchers
import com.xiwei.sujian.R
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import org.hamcrest.Matcher

object EditorCommitTextAction {
    fun commitText(text: String): ViewAction {
        return object : ViewAction {
            override fun getConstraints(): Matcher<View> {
                return ViewMatchers.withId(R.id.editor_content)
            }

            override fun getDescription(): String = "Commit text to SujianEditorView"

            override fun perform(uiController: UiController, view: View) {
                val editorView = view as? SujianEditorView
                    ?: throw AssertionError(
                        "EditorCommitTextAction: View with id editor_content is not a SujianEditorView, got ${view.javaClass.simpleName}"
                    )

                assert(editorView.visibility == View.VISIBLE) {
                    "EditorCommitTextAction: SujianEditorView is not VISIBLE (visibility=${editorView.visibility})"
                }

                assert(editorView.isEnabled) {
                    "EditorCommitTextAction: SujianEditorView is not enabled"
                }

                assert(editorView.isSessionBound) {
                    "EditorCommitTextAction: SujianEditorView does not have an editing session bound"
                }

                val focusOk = editorView.requestFocus()
                assert(focusOk) {
                    "EditorCommitTextAction: requestFocus() returned false"
                }

                uiController.loopMainThreadUntilIdle()

                val outAttrs = EditorInfo()
                val ic = editorView.onCreateInputConnection(outAttrs)
                assert(ic != null) {
                    "EditorCommitTextAction: onCreateInputConnection returned null"
                }

                val commitOk = ic!!.commitText(text, 1)
                assert(commitOk) {
                    "EditorCommitTextAction: commitText returned false"
                }

                uiController.loopMainThreadUntilIdle()
            }
        }
    }
}
