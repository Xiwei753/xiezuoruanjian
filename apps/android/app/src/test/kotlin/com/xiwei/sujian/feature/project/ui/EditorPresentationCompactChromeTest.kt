package com.xiwei.sujian.feature.project.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

/**
 * #640 A：窄屏 host 最终 chrome 决策回归测试。
 *
 * 窄屏 EditorPresentationHost 必须用最终 Editor chrome 测量：
 * - 绝不有 bottom primary NavigationBar（章节树阶段的底栏不参与 Editor 测量）；
 * - 不能使用 ChapterTree Scaffold 的 innerPadding（host 在 Scaffold 外，sibling）；
 * - presentationVisible=false 时背景透明（不遮盖 ProjectList/ChapterTree）；
 * - presentationVisible=true 时使用共享 editorSurfaceBackgroundColor，非 colorScheme.background。
 *
 * 决策由纯函数 [compactEditorChrome] 统一表达，生产 host 与单测共用。
 */
class EditorPresentationCompactChromeTest {
    @Test
    fun compactChrome_neverShowsPrimaryNavigation() {
        val visible = compactEditorChrome(presentationVisible = true)
        val hidden = compactEditorChrome(presentationVisible = false)
        assertFalse("可见 Editor 不得有 bottom NavigationBar", visible.showsPrimaryNavigation)
        assertFalse("预热 Editor 不得有 bottom NavigationBar", hidden.showsPrimaryNavigation)
    }

    @Test
    fun compactChrome_neverUsesChapterTreeInnerPadding() {
        val visible = compactEditorChrome(presentationVisible = true)
        val hidden = compactEditorChrome(presentationVisible = false)
        assertFalse("可见 Editor 不得用 ChapterTree innerPadding", visible.usesChapterTreeInnerPadding)
        assertFalse("预热 Editor 不得用 ChapterTree innerPadding", hidden.usesChapterTreeInnerPadding)
    }

    @Test
    fun compactChrome_hiddenUsesTransparentBackground() {
        val hidden = compactEditorChrome(presentationVisible = false)
        assertEquals(
            "预热阶段背景必须透明，不画 opaque editor surface",
            CompactEditorBackground.TRANSPARENT,
            hidden.background,
        )
    }

    @Test
    fun compactChrome_visibleUsesSharedEditorSurfaceBackground() {
        val visible = compactEditorChrome(presentationVisible = true)
        assertEquals(
            "可见阶段必须用共享 editorSurfaceBackgroundColor，非 colorScheme.background",
            CompactEditorBackground.SHARED_EDITOR_SURFACE,
            visible.background,
        )
    }
}
