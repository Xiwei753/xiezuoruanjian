package com.xiwei.sujian.support

import android.view.View
import android.view.inputmethod.EditorInfo
import androidx.test.espresso.UiController
import androidx.test.espresso.ViewAction
import androidx.test.espresso.matcher.ViewMatchers
import com.xiwei.sujian.R
import org.hamcrest.Matcher

object EditorCommitTextAction {
    fun commitText(text: String): ViewAction {
        return object : ViewAction {
            override fun getConstraints(): Matcher<View> {
                return ViewMatchers.withId(R.id.editor_content)
            }

            override fun getDescription(): String = "Commit text to SujianEditorView"

            override fun perform(uiController: UiController, view: View) {
                view.requestFocus()
                uiController.loopMainThreadUntilIdle()
                val outAttrs = EditorInfo()
                val ic = view.onCreateInputConnection(outAttrs)
                if (ic != null) {
                    ic.commitText(text, 1)
                }
                uiController.loopMainThreadUntilIdle()
            }
        }
    }
}
