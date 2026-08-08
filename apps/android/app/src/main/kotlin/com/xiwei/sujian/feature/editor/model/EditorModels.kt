package com.xiwei.sujian.feature.editor.model

/**
 * App 层编辑器枚举 — editor 子模块通过这些类型间接访问 UniFFI 绑定。
 *
 * 这些类型是 `uniffi.writer_core.*Dto` 的 app 层镜像，editor 子模块不得直接引用 UniFFI 绑定。
 * 映射在 [com.xiwei.sujian.core.interop.common.EditorDtoMapper] 中完成，pipeline 层负责转换。
 *
 *
 * 编辑事务原因 — 标识编辑操作的来源。
 *
 * 对应 UniFFI: `uniffi.writer_core.EditorTransactionCauseDto`
 */
enum class EditorTransactionCause {
    TYPING,
    DELETE,
    IME_COMPOSITION,
    TYPING_COMMIT,
    PASTE,
    UNDO,
    REDO,
    LOAD,
    FORMAT,
    PROGRAMMATIC,
}

/**
 * 编辑操作类型 — 标识编辑操作的类别。
 *
 * 对应 UniFFI: `uniffi.writer_core.EditorOperationKindDto`
 */
enum class EditorOperationKind {
    INSERT,
    DELETE,
    REPLACE,
    CURSOR_ONLY,
    COMPOSITION_UPDATE,
    COMPOSITION_COMMIT,
    COMPOSITION_CANCEL,
    LOAD,
    FORMAT,
}

/**
 * 动画模式 — 控制编辑器文字动画的粒度。
 *
 * 对应 UniFFI: `uniffi.writer_core.AnimationModeDto`
 */
enum class AnimationMode {
    GLYPH_ANIMATION,
    CLUSTER_ANIMATION,
    RUN_ANIMATION,
    LINE_REFLOW_ANIMATION,
    SNAPSHOT_ANIMATION,
    SYSTEM_SUPPRESSED,
}
