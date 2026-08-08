package com.xiwei.sujian.support

import android.os.Build
import android.view.View
import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.espresso.ViewAssertion
import com.xiwei.sujian.feature.editor.host.SujianEditorView
import org.junit.Assert

object EditorViewAssertions {
    fun hasDisplayText(expected: String): ViewAssertion {
        return ViewAssertion { view, noViewFoundException ->
            if (noViewFoundException != null) throw noViewFoundException
            val editorView =
                view as? SujianEditorView
                    ?: throw AssertionError(
                        "hasDisplayText: View is not a SujianEditorView, got ${view?.javaClass?.simpleName}",
                    )
            val actual = editorView.getDisplayText()
            Assert.assertEquals(
                "Editor display text mismatch",
                expected,
                actual,
            )
        }
    }

    fun hasSelectionUtf8(
        expectedStart: Int,
        expectedEnd: Int,
    ): ViewAssertion {
        return ViewAssertion { view, noViewFoundException ->
            if (noViewFoundException != null) throw noViewFoundException
            val editorView =
                view as? SujianEditorView
                    ?: throw AssertionError(
                        "hasSelectionUtf8: View is not a SujianEditorView, got ${view?.javaClass?.simpleName}",
                    )
            Assert.assertEquals(
                "UTF-8 selection start mismatch",
                expectedStart,
                editorView.getSelectionStart(),
            )
            Assert.assertEquals(
                "UTF-8 selection end mismatch",
                expectedEnd,
                editorView.getSelectionEnd(),
            )
        }
    }

    fun hasSelectionUtf16(
        expectedStart: Int,
        expectedEnd: Int,
    ): ViewAssertion {
        return ViewAssertion { view, noViewFoundException ->
            if (noViewFoundException != null) throw noViewFoundException
            val editorView =
                view as? SujianEditorView
                    ?: throw AssertionError(
                        "hasSelectionUtf16: View is not a SujianEditorView, got ${view?.javaClass?.simpleName}",
                    )
            Assert.assertEquals(
                "UTF-16 selection start mismatch",
                expectedStart,
                editorView.getSelectionStartUtf16(),
            )
            Assert.assertEquals(
                "UTF-16 selection end mismatch",
                expectedEnd,
                editorView.getSelectionEndUtf16(),
            )
        }
    }

    fun hasActionSetText(): ViewAssertion {
        return ViewAssertion { view, noViewFoundException ->
            if (noViewFoundException != null) throw noViewFoundException
            val editorView =
                view as? SujianEditorView
                    ?: throw AssertionError(
                        "hasActionSetText: View is not a SujianEditorView, got ${view?.javaClass?.simpleName}",
                    )
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP) {
                val nodeInfo = editorView.createAccessibilityNodeInfo()
                val hasAction =
                    nodeInfo.actionList.any {
                        it.id == AccessibilityNodeInfo.ACTION_SET_TEXT
                    }
                nodeInfo.recycle()
                Assert.assertTrue(
                    "SujianEditorView should expose ACTION_SET_TEXT as standard Android accessibility action",
                    hasAction,
                )
            }
        }
    }

    fun isSessionBound(): ViewAssertion {
        return ViewAssertion { view, noViewFoundException ->
            if (noViewFoundException != null) throw noViewFoundException
            val editorView =
                view as? SujianEditorView
                    ?: throw AssertionError(
                        "isSessionBound: View is not a SujianEditorView, got ${view?.javaClass?.simpleName}",
                    )
            Assert.assertTrue(
                "SujianEditorView should be session bound",
                editorView.isSessionBound,
            )
        }
    }

    fun isEditorReady(): ViewAssertion {
        return ViewAssertion { view, noViewFoundException ->
            if (noViewFoundException != null) throw noViewFoundException
            val editorView =
                view as? SujianEditorView
                    ?: throw AssertionError(
                        "isEditorReady: View is not a SujianEditorView, got ${view?.javaClass?.simpleName}",
                    )
            Assert.assertTrue(
                "SujianEditorView should be VISIBLE",
                editorView.visibility == View.VISIBLE,
            )
            Assert.assertTrue(
                "SujianEditorView should be session bound",
                editorView.isSessionBound,
            )
        }
    }
}
