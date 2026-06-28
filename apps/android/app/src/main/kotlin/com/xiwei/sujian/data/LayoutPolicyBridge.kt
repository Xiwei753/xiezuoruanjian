package com.xiwei.sujian.data

import android.util.Log
import com.xiwei.sujian.model.EditorMode
import com.xiwei.sujian.model.FoldPosture
import com.xiwei.sujian.model.HeightClass
import com.xiwei.sujian.model.LayoutPlan
import com.xiwei.sujian.model.NavigationMode
import com.xiwei.sujian.model.Orientation
import com.xiwei.sujian.model.PointerKind
import com.xiwei.sujian.model.ShellMode
import com.xiwei.sujian.model.WidthClass
import com.xiwei.sujian.model.WindowMetrics
import uniffi.writer_core.EditorModeDto
import uniffi.writer_core.FoldPostureDto
import uniffi.writer_core.HeightClassDto
import uniffi.writer_core.NavigationModeDto
import uniffi.writer_core.OrientationDto
import uniffi.writer_core.PointerKindDto
import uniffi.writer_core.ShellModeDto
import uniffi.writer_core.WidthClassDto

/**
 * Layout Policy Bridge — 通过 Core resolve_layout 获取跨端统一布局计划。
 *
 * Android 端测量窗口尺寸后传入 WindowMetrics，调用 Core 的 resolve_layout 获取 LayoutPlan。
 * 不允许 Android 端自己判断 isTablet 或自己发明断点。
 */
class LayoutPolicyBridge(private val appServiceBridge: AppServiceBridge) {

    companion object {
        private const val TAG = "LayoutPolicyBridge"
    }

    fun resolveLayout(metrics: WindowMetrics): LayoutPlan? {
        return try {
            val dto = uniffi.writer_core.WindowMetricsDto(
                widthVp = metrics.widthVp,
                heightVp = metrics.heightVp,
                safeTopVp = metrics.safeTopVp,
                safeBottomVp = metrics.safeBottomVp,
                keyboardVisible = metrics.keyboardVisible,
                foldPosture = metrics.foldPosture.toDto(),
                orientation = metrics.orientation.toDto(),
                pointer = metrics.pointer.toDto()
            )
            val result = appServiceBridge.resolveLayout(dto)
            when (result) {
                is BridgeResult.Success -> result.data.toModel()
                else -> null
            }
        } catch (e: Exception) {
            Log.e(TAG, "resolveLayout failed: ${e.message}", e)
            null
        }
    }

    // ── DTO conversion helpers ──

    private fun FoldPosture.toDto(): FoldPostureDto = when (this) {
        FoldPosture.Unknown -> FoldPostureDto.UNKNOWN
        FoldPosture.FullyOpened -> FoldPostureDto.FULLY_OPENED
        FoldPosture.HalfOpened -> FoldPostureDto.HALF_OPENED
        FoldPosture.Closed -> FoldPostureDto.CLOSED
    }

    private fun Orientation.toDto(): OrientationDto = when (this) {
        Orientation.Unknown -> OrientationDto.UNKNOWN
        Orientation.Portrait -> OrientationDto.PORTRAIT
        Orientation.Landscape -> OrientationDto.LANDSCAPE
    }

    private fun PointerKind.toDto(): PointerKindDto = when (this) {
        PointerKind.Unknown -> PointerKindDto.UNKNOWN
        PointerKind.Touch -> PointerKindDto.TOUCH
        PointerKind.Stylus -> PointerKindDto.STYLUS
        PointerKind.Mouse -> PointerKindDto.MOUSE
    }

    private fun WidthClassDto.toModel(): WidthClass = when (this) {
        WidthClassDto.COMPACT -> WidthClass.Compact
        WidthClassDto.MEDIUM -> WidthClass.Medium
        WidthClassDto.EXPANDED -> WidthClass.Expanded
    }

    private fun HeightClassDto.toModel(): HeightClass = when (this) {
        HeightClassDto.COMPACT -> HeightClass.Compact
        HeightClassDto.MEDIUM -> HeightClass.Medium
        HeightClassDto.EXPANDED -> HeightClass.Expanded
    }

    private fun ShellModeDto.toModel(): ShellMode = when (this) {
        ShellModeDto.SINGLE_PANE -> ShellMode.SinglePane
        ShellModeDto.SUPPORTING_PANE -> ShellMode.SupportingPane
        ShellModeDto.TWO_PANE -> ShellMode.TwoPane
    }

    private fun EditorModeDto.toModel(): EditorMode = when (this) {
        EditorModeDto.FULL_WIDTH -> EditorMode.FullWidth
        EditorModeDto.CENTERED_PAPER -> EditorMode.CenteredPaper
    }

    private fun NavigationModeDto.toModel(): NavigationMode = when (this) {
        NavigationModeDto.STACK -> NavigationMode.Stack
        NavigationModeDto.LIST_DETAIL -> NavigationMode.ListDetail
    }

    private fun uniffi.writer_core.LayoutPlanDto.toModel(): LayoutPlan = LayoutPlan(
        widthClass = widthClass.toModel(),
        heightClass = heightClass.toModel(),
        shellMode = shellMode.toModel(),
        editorMode = editorMode.toModel(),
        navigationMode = navigationMode.toModel(),
        contentMaxWidthVp = contentMaxWidthVp,
        pagePaddingVp = pagePaddingVp,
        gridColumns = gridColumns.toInt(),
        showSidePanel = showSidePanel,
        showBottomBar = showBottomBar,
        sidePanelWidthVp = sidePanelWidthVp,
        primaryPaneWeight = primaryPaneWeight,
        detailPanelMaxWidthVp = detailPanelMaxWidthVp
    )
}
