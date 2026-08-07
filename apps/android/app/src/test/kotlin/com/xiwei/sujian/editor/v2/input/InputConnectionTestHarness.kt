package com.xiwei.sujian.editor.v2.input

import android.content.Context
import android.view.View
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror

/**
 * Builds the production input stack ([DisplayTextMirror] + [FakeInputCommandPort] +
 * [AndroidInputAdapter] + [AndroidInputConnection]) exactly as the editor wires it.
 *
 * The host view's context is the Robolectric application context; InputMethodManager
 * lookups return the framework instance carrying [RecordingInputMethodManagerShadow]
 * (see the `@Config(shadows = ...)` requirement on every test class using this harness).
 * [imm] exposes the shadow's updateSelection counter for IME-notification assertions.
 */
class InputConnectionTestHarness(
    text: String = "ABXY",
    cursorUtf8: Int = 4,
) {
    val imm = RecordingInputMethodManagerShadow
    val context: Context = ApplicationProvider.getApplicationContext()
    val mirror = DisplayTextMirror()
    val commandPort: FakeInputCommandPort
    val adapter: AndroidInputAdapter
    val hostView: View
    val connection: AndroidInputConnection

    init {
        RecordingInputMethodManagerShadow.resetUpdateSelectionCount()
        mirror.loadText(text, cursorUtf8)
        commandPort = FakeInputCommandPort(mirror, text, cursorUtf8)
        adapter = AndroidInputAdapter(mirror, commandPort, null)
        hostView = View(context)
        adapter.setHostView(hostView)
        connection = AndroidInputConnection(adapter, mirror, commandPort, hostView, null)
    }
}
