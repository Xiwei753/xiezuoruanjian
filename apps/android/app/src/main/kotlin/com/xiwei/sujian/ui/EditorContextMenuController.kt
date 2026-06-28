package com.xiwei.sujian.ui

import android.view.ActionMode
import android.view.Menu
import android.view.MenuItem

/**
 * EditorContextMenuController - 编辑器上下文菜单控制器
 *
 * 动作接管层，不重写系统菜单
 * 通过 ActionMode.Callback 拦截系统菜单动作，
 * 将选区操作（复制/剪切/粘贴/全选等）路由到写作逻辑，
 * 后续可扩展自定义菜单项和 AI 辅助
 *
 * ## 当前阶段
 * - WriterEditText → EditorContextMenuController → ActionMode.Callback
 *
 * ## 架构
 * - 仅接管动作（通过 EditorViewModel 路由）
 * - 未接管菜单外观（保留 WriterEditText 系统菜单样式）
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
                    // 剪切需要禁用动画 - 避免动画残留
                    editor.runWithoutTextAnimations {
                        // performCut 会删除文本，需要禁用动画
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
                // 当前阶段：保留系统菜单项，不修改
                // 后续可在此处添加/移除自定义菜单项
                return true
            }

            override fun onPrepareActionMode(mode: ActionMode?, menu: Menu?): Boolean {
                // 当前阶段：保留系统菜单外观
                // 后续可在此处清理默认菜单项
                return true
            }

            override fun onActionItemClicked(mode: ActionMode?, item: MenuItem?): Boolean {
                // 当前阶段：委托给 handleTextContextMenuItem 处理
                // 后续可在此处拦截自定义菜单项点击
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