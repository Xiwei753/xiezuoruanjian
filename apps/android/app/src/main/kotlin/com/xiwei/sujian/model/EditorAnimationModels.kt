package com.xiwei.sujian.model

/**
 * Android typed model for editor animation events from Core.
 *
 * Desktop QML may keep consuming animation_events_json for its overlay path; Android must stay on
 * typed models and must not route typed DTO -> JSON -> handwritten parser.
 */
enum class EditorAnimationKindData {
    Insert,
    Delete,
    Cursor
}

/** Legacy: 仅用于旧版 WriterEditText fallback，自研写作区（SujianEditorView）不再使用此链路 */
data class EditorAnimationEventData(
    val id: ULong,
    val kind: EditorAnimationKindData,
    val rangeStart: Int,
    val rangeLen: Int,
    val text: String,
    val oldCursorIndex: Int,
    val newCursorIndex: Int,
    val durationMs: Long
)

// ── Visual Transaction 数据模型 (Phase 2) ──

/**
 * 编辑原因枚举，与 Core EditorTransactionCauseDto 对齐。
 *
 * 包含 Undo/Redo，用于视觉事务的 cause 映射。
 */
enum class SujianEditCauseData {
    Typing,
    Delete,
    ImeComposition,
    TypingCommit,
    Paste,
    Undo,
    Redo,
    Load,
    Format,
    Programmatic
}

/**
 * 坐标模式枚举，与 Core VisualCoordinateModeDto 对齐。
 */
enum class VisualCoordinateModeData {
    Baseline
}

/**
 * 视觉事务数据类，与 Core EditorVisualTransactionDto 对齐。
 *
 * Core 层只裁判事件语义和范围（UTF-8 byte offset），
 * Android 层负责 layout 坐标转换和绘制。
 *
 * 坐标字段（oldCursorRect, newCursorRect, deletedGlyphRects, insertGlyphRects）
 * 由 Android 层自行填充——Core 不传坐标。
 */
data class EditorVisualTransactionData(
    val id: ULong,
    val kind: EditorAnimationKindData,
    val cause: SujianEditCauseData,
    val oldText: String,
    val newText: String,
    /** 旧选区 anchor（UTF-8 byte offset） */
    val oldSelectionAnchor: Int,
    /** 旧选区 head（UTF-8 byte offset） */
    val oldSelectionHead: Int,
    /** 新选区 anchor（UTF-8 byte offset） */
    val newSelectionAnchor: Int,
    /** 新选区 head（UTF-8 byte offset） */
    val newSelectionHead: Int,
    /** 插入范围起始（UTF-8 byte offset），无插入时为 0 */
    val insertedRangeStart: Int,
    /** 插入范围结束（UTF-8 byte offset），无插入时为 0 */
    val insertedRangeEnd: Int,
    val durationMs: Long,
    val coordinateMode: VisualCoordinateModeData,

    // ── 可变坐标字段（由 Android 层填充） ──
    /** 插入前光标矩形，由 runVisualEdit 在 edit 前捕获 */
    var oldCursorRect: SujianCursorRectData? = null,
    /** 插入后光标矩形，由 runVisualEdit 在 edit 后捕获 */
    var newCursorRect: SujianCursorRectData? = null,
    /** 被删除的 glyph 矩形列表，由 onBeforeDelete 捕获 */
    var deletedGlyphRects: List<SujianGlyphRectData> = emptyList(),
    /** 被插入的 glyph 矩形列表，由 handleInsertTransaction 填充 */
    var insertGlyphRects: List<SujianGlyphRectData> = emptyList(),
    /** 受局部 reflow 影响的 glyph 旧/新位置列表，由 runVisualEdit 填充 */
    var reflowGlyphRects: List<SujianReflowGlyphRectData> = emptyList()
)

/**
 * 光标矩形数据类，与 Core CursorRectDto 对齐。
 */
data class SujianCursorRectData(
    val x: Double,
    val top: Double,
    val bottom: Double,
    val baselineY: Double
)

/**
 * Glyph 矩形数据类，与 Core GlyphRectDto 对齐。
 */
data class SujianGlyphRectData(
    val x: Double,
    val y: Double,
    val w: Double,
    val h: Double,
    val char: String,
    val baselineY: Double
)

/**
 * 受局部 reflow 影响的 glyph 的旧位置和新位置，与 Core ReflowGlyphRect 对齐。
 *
 * 中间插入时，插入点右侧的文字需要做轻量位移动画（局部挤开），
 * 避免瞬间大跳。SujianReflowGlyphRectData 记录这些 glyph 在插入前后的位置。
 *
 * 只影响同一行中插入点右侧的 glyph，以及受影响的相邻 1-2 行。
 * 超过 2 行、跨段落、滚动中、格式化中、加载中时直接 snap，不收集。
 */
data class SujianReflowGlyphRectData(
    /** 该 glyph 对应的字符 */
    val char: String,
    /** 该 glyph 在新文本中的 UTF-16 起始偏移（用于静态层跳过 reflow range） */
    val byteStart: Int,
    /** 该 glyph 在新文本中的 UTF-16 结束偏移 */
    val byteEnd: Int,
    /** 插入前的 x 坐标（文档坐标系，不含 scroll offset） */
    val oldX: Double,
    /** 插入前的 y 坐标（文档坐标系，不含 scroll offset） */
    val oldY: Double,
    /** 插入前的基线 Y 坐标 */
    val oldBaselineY: Double,
    /** 插入后的 x 坐标（文档坐标系，不含 scroll offset） */
    val newX: Double,
    /** 插入后的 y 坐标（文档坐标系，不含 scroll offset） */
    val newY: Double,
    /** 插入后的基线 Y 坐标 */
    val newBaselineY: Double,
    /** glyph 宽度 */
    val w: Double,
    /** glyph 高度 */
    val h: Double,
    /** 所在 visual line 索引（新布局中的索引） */
    val lineIndex: Int
)

/**
 * 视觉编辑上下文，由 runVisualEdit 生成并传给 AnimationController。
 *
 * 包含编辑前后的完整快照：文本内容、选区位置（UTF-16 offset）、光标矩形。
 * fetchVisualTransaction 必须使用此快照，不许再从当前 buffer 取 old/new，
 * 否则 Core 无法算出真实的 Insert/Delete。
 */
data class SujianVisualEditContext(
    val oldText: String,
    val newText: String,
    val oldSelectionAnchor: Int,  // UTF-16 offset in oldText
    val oldSelectionHead: Int,    // UTF-16 offset in oldText
    val newSelectionAnchor: Int,  // UTF-16 offset in newText
    val newSelectionHead: Int,    // UTF-16 offset in newText
    val oldCursorRect: SujianCursorRectData?,
    val newCursorRect: SujianCursorRectData?,
    val cause: SujianEditCauseData,
    /** 受局部 reflow 影响的 glyph 旧/新位置列表，由 runVisualEdit 填充 */
    val reflowGlyphRects: List<SujianReflowGlyphRectData> = emptyList()
)
