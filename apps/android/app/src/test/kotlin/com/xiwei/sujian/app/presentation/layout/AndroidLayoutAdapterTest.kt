package com.xiwei.sujian.app.presentation.layout

import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.writer_core.WorkspacePaneModeDto

/**
 * #628：AndroidLayoutAdapter 的纯函数部分 — Core WorkspacePaneMode → Material3 partitions 映射。
 *
 * #628 删除了 buildCapabilities / WindowCapabilitiesDto / PointerClassDto /
 * AndroidNavigationPresentation / availablePaneCount — 断点与壳层模式改由 Rust
 * `presentation/layout` 决定，Android 不再做窗口能力判断，因此旧
 * AndroidAdaptiveLayoutPolicyTest 中针对这些函数的测试一并删除。
 * 仅保留 [maxHorizontalPartitionsFor] 的映射测试（控件映射，非断点判断）。
 */
class AndroidLayoutAdapterTest {
    @Test
    fun `workspace pane mode maps to max horizontal partitions`() {
        assertEquals(1, maxHorizontalPartitionsFor(WorkspacePaneModeDto.SINGLE_PANE))
        assertEquals(2, maxHorizontalPartitionsFor(WorkspacePaneModeDto.LIST_DETAIL))
        assertEquals(3, maxHorizontalPartitionsFor(WorkspacePaneModeDto.THREE_PANE))
    }
}
