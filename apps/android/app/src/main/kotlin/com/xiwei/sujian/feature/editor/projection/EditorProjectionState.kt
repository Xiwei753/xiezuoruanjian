package com.xiwei.sujian.feature.editor.projection

/**
 * 视口锚点 — 用逻辑位置（文本偏移 + 行内相对比例）保存滚动状态，
 * 替代绝对像素（scrollX/scrollY）。在字体/字号/行距变化时仍能正确恢复，
 * 因为恢复时用新行高乘以 fraction，不依赖旧布局的绝对像素。
 *
 * @property textOffsetUtf16 锚点文本偏移（UTF-16 码元）
 * @property offsetWithinLineFraction 锚点在行内的纵向相对比例（0.0 = 行顶, 1.0 = 行底）
 */
data class ViewportAnchor(
    val textOffsetUtf16: Int,
    val offsetWithinLineFraction: Float,
)

/**
 * #592 三：业务级关闭原因 — 由 workspace 导航事件明确给出，不能从
 * DisposableEffect 推断业务对象是否结束。
 */
enum class SessionCloseReason {
    /** 用户从正文返回章节列表/作品列表（workspace 导航离开 Editor 目的地）。 */
    WORKSPACE_NAVIGATION,

    /** 章节切换（旧章节 session 关闭，新章节新建 session）。 */
    CHAPTER_SWITCH,

    /** 章节/作品被删除。 */
    DELETE,
}

/**
 * #592 四：会话层持有的纯数据投影状态 — 不含 View、TextPaint、FrameClock、
 * Rect/Transform 或 Compose mutableState。窗口层在销毁/附着时读写。
 * #595 九：仅保留滚动位置（配置变化/返回重进时恢复 View 滚动），
 * 字体/主题/视口等视觉配置由 Compose 主题和 profile 权威提供，不再保存。
 *
 * #630 R14：用逻辑视口锚点替代绝对 scrollX/scrollY 像素。
 * 不再保存绝对像素，只保存 [viewportAnchor]；窗口层恢复时用文本偏移 + 行内像素
 * 找回新布局中的滚动位置，避免字体/字号/排版变化时同一绝对 Y 不再对应同一段文字。
 */
data class ProjectionSnapshot(
    val viewportAnchor: ViewportAnchor? = null,
)
