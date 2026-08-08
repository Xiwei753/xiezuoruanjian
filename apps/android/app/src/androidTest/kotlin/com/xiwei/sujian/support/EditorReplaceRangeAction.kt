package com.xiwei.sujian.support

import android.view.View
import androidx.test.espresso.UiController
import androidx.test.espresso.ViewAction
import androidx.test.espresso.matcher.ViewMatchers
import com.xiwei.sujian.R
import com.xiwei.sujian.feature.editor.host.SujianEditorView
import org.hamcrest.Matcher
import org.junit.Assert
import uniffi.writer_core.EditorTransactionCauseDto

object EditorReplaceRangeAction {
    fun replaceRange(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        originalText: String,
        cause: EditorTransactionCauseDto = EditorTransactionCauseDto.TYPING,
    ): ViewAction {
        return object : ViewAction {
            override fun getConstraints(): Matcher<View> {
                return ViewMatchers.withId(R.id.editor_content)
            }

            override fun getDescription(): String = "Replace range in SujianEditorView via replaceRangeTyped"

            override fun perform(
                uiController: UiController,
                view: View,
            ) {
                val editorView =
                    view as? SujianEditorView
                        ?: throw AssertionError(
                            "EditorReplaceRangeAction: View is not a SujianEditorView, " +
                                "got ${view.javaClass.simpleName}",
                        )

                Assert.assertTrue(
                    "EditorReplaceRangeAction: SujianEditorView is not session bound",
                    editorView.isSessionBound,
                )

                editorView.replaceRangeTyped(byteStart, byteEndExclusive, replacementText, originalText, cause)

                uiController.loopMainThreadUntilIdle()
            }
        }
    }
}
