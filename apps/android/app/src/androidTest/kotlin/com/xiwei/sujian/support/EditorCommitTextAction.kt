package com.xiwei.sujian.support

import android.view.View
import android.view.inputmethod.EditorInfo
import androidx.test.espresso.UiController
import androidx.test.espresso.ViewAction
import androidx.test.espresso.matcher.ViewMatchers
import com.xiwei.sujian.R
import com.xiwei.sujian.feature.editor.host.SujianEditorView
import org.hamcrest.Matcher
import org.junit.Assert

object EditorCommitTextAction {
    fun commitText(text: String): ViewAction {
        return object : ViewAction {
            override fun getConstraints(): Matcher<View> {
                return ViewMatchers.withId(R.id.editor_content)
            }

            override fun getDescription(): String = "Commit text to SujianEditorView"

            override fun perform(
                uiController: UiController,
                view: View,
            ) {
                val editorView =
                    view as? SujianEditorView
                        ?: throw AssertionError(
                            "EditorCommitTextAction: View with id editor_content is not a SujianEditorView, " +
                                "got ${view.javaClass.simpleName}",
                        )

                Assert.assertTrue(
                    "EditorCommitTextAction: SujianEditorView is not VISIBLE (visibility=${editorView.visibility})",
                    editorView.visibility == View.VISIBLE,
                )

                Assert.assertTrue(
                    "EditorCommitTextAction: SujianEditorView is not enabled",
                    editorView.isEnabled,
                )

                Assert.assertTrue(
                    "EditorCommitTextAction: SujianEditorView does not have an editing session bound",
                    editorView.isSessionBound,
                )

                val focusOk = editorView.requestFocus()
                Assert.assertTrue(
                    "EditorCommitTextAction: requestFocus() returned false",
                    focusOk,
                )

                uiController.loopMainThreadUntilIdle()

                val outAttrs = EditorInfo()
                val ic = editorView.onCreateInputConnection(outAttrs)
                Assert.assertNotNull(
                    "EditorCommitTextAction: onCreateInputConnection returned null",
                    ic,
                )

                val commitOk = ic!!.commitText(text, 1)
                Assert.assertTrue(
                    "EditorCommitTextAction: commitText returned false",
                    commitOk,
                )

                uiController.loopMainThreadUntilIdle()
            }
        }
    }
}
