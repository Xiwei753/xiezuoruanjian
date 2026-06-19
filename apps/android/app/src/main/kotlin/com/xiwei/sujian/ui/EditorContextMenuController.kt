package com.xiwei.sujian.ui

import android.view.ActionMode
import android.view.Menu
import android.view.MenuItem

/**
 * EditorContextMenuController - ?????????????
 *
 * ??????,???????
 * ?? ActionMode.Callback ???????????,
 * ??????(?????????????????),
 * ???????????????AI ????
 *
 * ## ????
 * - WriterEditText ? EditorContextMenuController ? ActionMode.Callback
 *
 * ## ??
 * - ??????(??? EditorViewModel ??)
 * - ???????(??? WriterEditText ???????)
 */
class EditorContextMenuController(
    private val editor: WriterEditText
) {

    fun handleTextContextMenuItem(id: Int): Boolean {
        return when (id) {
            android.R.id.selectAll -> {
                editor.selectAll()
                true
            }
            android.R.id.copy -> {
                val selStart = editor.selectionStart
                val selEnd = editor.selectionEnd
                if (selStart != selEnd) {
                    editor.performCopy()
                }
                true
            }
            android.R.id.cut -> {
                val selStart = editor.selectionStart
                val selEnd = editor.selectionEnd
                if (selStart != selEnd) {
                    // ?????????? - ???????
                    editor.runWithoutTextAnimations {
                        // performCut ?????,????????
                        editor.onPerformCut()
                    }
                }
                true
            }
            android.R.id.paste,
            android.R.id.pasteAsPlainText -> {
                editor.runWithoutTextAnimations {
                    editor.performPasteFromSystem(id)
                }
                true
            }
            else -> false
        }
    }

    fun installActionModeCallbacks() {
        editor.customSelectionActionModeCallback = createSelectionCallback()
        editor.customInsertionActionModeCallback = createInsertionCallback()
    }

    private fun createSelectionCallback(): ActionMode.Callback {
        return object : ActionMode.Callback {
            override fun onCreateActionMode(mode: ActionMode?, menu: Menu?): Boolean {
                // ????:?????????,?????
                // ???????????????????????
                return true
            }

            override fun onPrepareActionMode(mode: ActionMode?, menu: Menu?): Boolean {
                // ????:????????
                // ????????????????
                return true
            }

            override fun onActionItemClicked(mode: ActionMode?, item: MenuItem?): Boolean {
                // ????:????? handleTextContextMenuItem ??
                // ??????????????????
                return false
            }

            override fun onDestroyActionMode(mode: ActionMode?) {
                // No-op
            }
        }
    }

    private fun createInsertionCallback(): ActionMode.Callback {
        return object : ActionMode.Callback {
            override fun onCreateActionMode(mode: ActionMode?, menu: Menu?): Boolean {
                return true
            }

            override fun onPrepareActionMode(mode: ActionMode?, menu: Menu?): Boolean {
                return true
            }

            override fun onActionItemClicked(mode: ActionMode?, item: MenuItem?): Boolean {
                return false
            }

            override fun onDestroyActionMode(mode: ActionMode?) {
                // No-op
            }
        }
    }
}