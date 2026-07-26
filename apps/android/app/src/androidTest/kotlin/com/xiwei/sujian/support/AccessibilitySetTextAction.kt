package com.xiwei.sujian.support

import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.espresso.UiController
import androidx.test.espresso.ViewAction
import androidx.test.espresso.matcher.ViewMatchers
import com.xiwei.sujian.R
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import org.hamcrest.Matcher
import org.junit.Assert

object AccessibilitySetTextAction {
    fun setText(text: String): ViewAction {
        return object : ViewAction {
            override fun getConstraints(): Matcher<View> {
                return ViewMatchers.withId(R.id.editor_content)
            }

            override fun getDescription(): String =
                "Set text on SujianEditorView via ACTION_SET_TEXT accessibility action"

            override fun perform(uiController: UiController, view: View) {
                if (Build.VERSION.SDK_INT < Build.VERSION_CODES.LOLLIPOP) {
                    throw AssertionError("ACTION_SET_TEXT requires API 21+")
                }

                val editorView = view as? SujianEditorView
                    ?: throw AssertionError(
                        "AccessibilitySetTextAction: View with id editor_content is not a SujianEditorView, got ${view.javaClass.simpleName}"
                    )

                Assert.assertTrue(
                    "AccessibilitySetTextAction: SujianEditorView is not VISIBLE (visibility=${editorView.visibility})",
                    editorView.visibility == View.VISIBLE
                )

                Assert.assertTrue(
                    "AccessibilitySetTextAction: SujianEditorView is not enabled",
                    editorView.isEnabled
                )

                Assert.assertTrue(
                    "AccessibilitySetTextAction: SujianEditorView does not have an editing session bound",
                    editorView.isSessionBound
                )

                val args = Bundle()
                args.putCharSequence(
                    AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE,
                    text
                )
                val result = editorView.performAccessibilityAction(
                    AccessibilityNodeInfo.ACTION_SET_TEXT, args
                )
                Assert.assertTrue(
                    "AccessibilitySetTextAction: performAccessibilityAction(ACTION_SET_TEXT) returned false for text='$text'",
                    result
                )

                uiController.loopMainThreadUntilIdle()
            }
        }
    }
}
