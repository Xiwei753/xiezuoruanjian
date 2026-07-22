package com.xiwei.sujian.editor.v2.pipeline

import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.coordinator.EditableTextTarget
import com.xiwei.sujian.editor.v2.coordinator.TextEditorProfile
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

class DisplayHost(
    val view: SujianEditorView,
    val pipeline: AndroidEditorPipeline
) {
    val mirror: DisplayTextMirror get() = pipeline.mirror

    fun bindSession(
        sessionBridge: com.xiwei.sujian.editor.v2.host.EditorKernelBridge,
        profile: TextEditorProfile,
        initialText: String,
        initialCursorUtf8: Int
    ) {
        view.bindSession(sessionBridge, profile, initialText, initialCursorUtf8)
    }

    fun unbindSession(reason: String) {
        view.unbindSession(reason)
    }

    fun loadText(text: String, cursorUtf8: Int) {
        view.loadText(text, cursorUtf8)
    }

    fun getText(): String = view.getText()

    fun setSearchHighlights(highlights: List<Pair<Int, Int>>) {
        view.setSearchHighlights(highlights)
    }

    fun clearSearchHighlights() {
        view.clearSearchHighlights()
    }

    fun requestFocus() {
        view.requestFocus()
    }

    fun release() {
        view.release()
    }

    fun resetForReuse() {
        view.resetForReuse()
    }
}
