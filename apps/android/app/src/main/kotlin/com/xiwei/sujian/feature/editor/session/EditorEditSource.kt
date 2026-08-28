package com.xiwei.sujian.feature.editor.session

/**
 * #595 二/四：编辑来源标记 — 区分普通输入、撤销/恢复、程序化替换。
 * 由 pipeline 命令与 [PipelineOutput.Edited.source] 天然携带，
 * 不再使用 View 上的可变侧信道标记。
 */
enum class EditorEditSource {
    NORMAL,
    UNDO,
    REDO,
    PROGRAMMATIC,
}
