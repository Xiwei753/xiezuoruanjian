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

object EditorCompositionAction {
    fun setComposingText(
        text: String,
        newCursorPosition: Int = 1,
    ): ViewAction {
        return object : ViewAction {
            override fun getConstraints(): Matcher<View> {
                return ViewMatchers.withId(R.id.editor_content)
            }

            override fun getDescription(): String = "Set composing text on SujianEditorView"

            override fun perform(
                uiController: UiController,
                view: View,
            ) {
                val editorView =
                    view as? SujianEditorView
                        ?: throw AssertionError(
                            "EditorCompositionAction: View is not a SujianEditorView, got ${view.javaClass.simpleName}",
                        )

                Assert.assertTrue(
                    "EditorCompositionAction: SujianEditorView is not VISIBLE",
                    editorView.visibility == View.VISIBLE,
                )

                Assert.assertTrue(
                    "EditorCompositionAction: SujianEditorView is not session bound",
                    editorView.isSessionBound,
                )

                val focusOk = editorView.requestFocus()
                Assert.assertTrue("requestFocus() returned false", focusOk)

                uiController.loopMainThreadUntilIdle()

                val outAttrs = EditorInfo()
                val ic = editorView.onCreateInputConnection(outAttrs)
                Assert.assertNotNull("onCreateInputConnection returned null", ic)

                val setOk = ic!!.setComposingText(text, newCursorPosition)
                Assert.assertTrue("setComposingText returned false", setOk)

                uiController.loopMainThreadUntilIdle()
            }
        }
    }

    fun finishComposingText(): ViewAction {
        return object : ViewAction {
            override fun getConstraints(): Matcher<View> {
                return ViewMatchers.withId(R.id.editor_content)
            }

            override fun getDescription(): String = "Finish composing text on SujianEditorView"

            override fun perform(
                uiController: UiController,
                view: View,
            ) {
                val editorView =
                    view as? SujianEditorView
                        ?: throw AssertionError(
                            "EditorCompositionAction: View is not a SujianEditorView, got ${view.javaClass.simpleName}",
                        )

                val focusOk = editorView.requestFocus()
                Assert.assertTrue("requestFocus() returned false", focusOk)

                uiController.loopMainThreadUntilIdle()

                val outAttrs = EditorInfo()
                val ic = editorView.onCreateInputConnection(outAttrs)
                Assert.assertNotNull("onCreateInputConnection returned null", ic)

                ic!!.finishComposingText()

                uiController.loopMainThreadUntilIdle()
            }
        }
    }

    fun commitTextViaInputConnection(text: String): ViewAction {
        return object : ViewAction {
            override fun getConstraints(): Matcher<View> {
                return ViewMatchers.withId(R.id.editor_content)
            }

            override fun getDescription(): String = "Commit text via InputConnection on SujianEditorView"

            override fun perform(
                uiController: UiController,
                view: View,
            ) {
                val editorView =
                    view as? SujianEditorView
                        ?: throw AssertionError(
                            "EditorCompositionAction: View is not a SujianEditorView, got ${view.javaClass.simpleName}",
                        )

                val focusOk = editorView.requestFocus()
                Assert.assertTrue("requestFocus() returned false", focusOk)

                uiController.loopMainThreadUntilIdle()

                val outAttrs = EditorInfo()
                val ic = editorView.onCreateInputConnection(outAttrs)
                Assert.assertNotNull("onCreateInputConnection returned null", ic)

                val commitOk = ic!!.commitText(text, 1)
                Assert.assertTrue("commitText returned false", commitOk)

                uiController.loopMainThreadUntilIdle()
            }
        }
    }

    fun sendKeyDelete(): ViewAction {
        return object : ViewAction {
            override fun getConstraints(): Matcher<View> {
                return ViewMatchers.withId(R.id.editor_content)
            }

            override fun getDescription(): String = "Send delete key to SujianEditorView"

            override fun perform(
                uiController: UiController,
                view: View,
            ) {
                val editorView =
                    view as? SujianEditorView
                        ?: throw AssertionError(
                            "EditorCompositionAction: View is not a SujianEditorView, got ${view.javaClass.simpleName}",
                        )

                val focusOk = editorView.requestFocus()
                Assert.assertTrue("requestFocus() returned false", focusOk)

                uiController.loopMainThreadUntilIdle()

                val outAttrs = EditorInfo()
                val ic = editorView.onCreateInputConnection(outAttrs)
                Assert.assertNotNull("onCreateInputConnection returned null", ic)

                ic!!.sendKeyEvent(
                    android.view.KeyEvent(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.KEYCODE_DEL),
                )
                ic.sendKeyEvent(
                    android.view.KeyEvent(android.view.KeyEvent.ACTION_UP, android.view.KeyEvent.KEYCODE_DEL),
                )

                uiController.loopMainThreadUntilIdle()
            }
        }
    }
}
