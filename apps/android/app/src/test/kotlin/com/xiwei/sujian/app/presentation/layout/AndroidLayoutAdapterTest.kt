package com.xiwei.sujian.app.presentation.layout

import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.writer_core.WorkspaceLayoutModeDto

/**
 * #628：AndroidLayoutAdapter 的纯函数部分 — Core WorkspaceLayoutMode → Kotlin 枚举映射。
 *
 * #628 删除了 buildCapabilities / WindowCapabilitiesDto / PointerClassDto /
 * AndroidNavigationPresentation / availablePaneCount / maxHorizontalPartitionsFor —
 * 断点与壳层模式改由 Rust `presentation/layout` 决定，Android 不再做窗口能力判断，
 * 也不再做 WorkspacePaneMode → maxHorizontalPartitions 的控件映射（Material3
 * PaneScaffoldDirective 整条死链已删除）。仅保留 [toWorkspaceLayoutMode]
 * 的枚举映射测试（interop 映射，非断点判断）。
 */
class AndroidLayoutAdapterTest {
    @Test
    fun `workspace layout mode maps to kotlin enum`() {
        assertEquals(
            WorkspaceLayoutMode.SINGLE_PANE,
            WorkspaceLayoutModeDto.SINGLE_PANE.toWorkspaceLayoutMode(),
        )
        assertEquals(
            WorkspaceLayoutMode.WORKBENCH,
            WorkspaceLayoutModeDto.WORKBENCH.toWorkspaceLayoutMode(),
        )
    }
}
