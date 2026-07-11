package com.xiwei.sujian.data

import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.model.*
import uniffi.writer_core.*

class LayoutPolicyBridge internal constructor(private val holder: WriterAppServiceHolder) {

    companion object {
        private const val TAG = "LayoutPolicyBridge"
    }

    fun resolveLayout(metrics: WindowMetricsDto): BridgeResult<LayoutPlanDto> = holder.wrapResult {
        holder.service.resolveLayout(metrics)
    }

    fun resolveScreenPolicy(screenRole: ScreenRoleDto, shellMode: ShellModeDto): BridgeResult<ScreenPolicyDto> = holder.wrapResult {
        holder.service.resolveScreenPolicy(screenRole, shellMode)
    }

    fun resolveLayout(metrics: WindowMetrics): LayoutPlan? {
        return try {
            val dto = WindowMetricsDto(
                widthDp = metrics.widthDp,
                heightDp = metrics.heightDp,
                safeTopDp = metrics.safeTopDp,
                safeBottomDp = metrics.safeBottomDp,
                keyboardVisible = metrics.keyboardVisible,
                foldFeature = metrics.foldFeature.toDto(),
                orientation = metrics.orientation.toDto(),
                pointer = metrics.pointer.toDto()
            )
            val result = resolveLayout(dto)
            when (result) {
                is BridgeResult.Success -> result.data.toModel()
                else -> null
            }
        } catch (e: Exception) {
            DiagnosticsLogger.e(TAG, "resolveLayout failed: ${e.message}", e)
            null
        }
    }

    private fun FoldFeatureInfo.toDto(): FoldFeatureInfoDto = FoldFeatureInfoDto(
        state = state.toDto(),
        orientation = orientation.toDto(),
        isSeparating = isSeparating,
        occlusion = occlusion.toDto(),
        boundsLeftVp = boundsLeftVp,
        boundsTopVp = boundsTopVp,
        boundsRightVp = boundsRightVp,
        boundsBottomVp = boundsBottomVp
    )

    private fun FoldState.toDto(): FoldStateDto = when (this) {
        FoldState.None -> FoldStateDto.NONE
        FoldState.Flat -> FoldStateDto.FLAT
        FoldState.HalfOpened -> FoldStateDto.HALF_OPENED
    }

    private fun FoldOrientation.toDto(): FoldOrientationDto = when (this) {
        FoldOrientation.Horizontal -> FoldOrientationDto.HORIZONTAL
        FoldOrientation.Vertical -> FoldOrientationDto.VERTICAL
    }

    private fun FoldOcclusion.toDto(): FoldOcclusionDto = when (this) {
        FoldOcclusion.None -> FoldOcclusionDto.NONE
        FoldOcclusion.Full -> FoldOcclusionDto.FULL
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
        WidthClassDto.LARGE -> WidthClass.Large
        WidthClassDto.EXTRA_LARGE -> WidthClass.ExtraLarge
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
        ShellModeDto.THREE_PANE -> ShellMode.ThreePane
    }

    private fun EditorModeDto.toModel(): EditorMode = when (this) {
        EditorModeDto.FULL_WIDTH -> EditorMode.FullWidth
        EditorModeDto.CENTERED_PAPER -> EditorMode.CenteredPaper
    }

    private fun NavigationModeDto.toModel(): NavigationMode = when (this) {
        NavigationModeDto.STACK -> NavigationMode.Stack
        NavigationModeDto.LIST_DETAIL -> NavigationMode.ListDetail
    }

    private fun NavigationPresentationDto.toModel(): NavigationPresentation = when (this) {
        NavigationPresentationDto.BOTTOM_BAR -> NavigationPresentation.BottomBar
        NavigationPresentationDto.NAVIGATION_RAIL -> NavigationPresentation.NavigationRail
        NavigationPresentationDto.PERMANENT_DRAWER -> NavigationPresentation.PermanentDrawer
    }

    private fun WorkspacePaneModeDto.toModel(): WorkspacePaneMode = when (this) {
        WorkspacePaneModeDto.SINGLE_PANE -> WorkspacePaneMode.SinglePane
        WorkspacePaneModeDto.LIST_DETAIL -> WorkspacePaneMode.ListDetail
        WorkspacePaneModeDto.THREE_PANE -> WorkspacePaneMode.ThreePane
    }

    private fun VisiblePaneRolesDto.toModel(): VisiblePaneRoles = VisiblePaneRoles(
        showProjectList = showProjectList,
        showChapterTree = showChapterTree,
        showEditor = showEditor,
        showSupporting = showSupporting
    )

    private fun PaneWidthConstraintDto.toModel(): PaneWidthConstraint = PaneWidthConstraint(
        minDp = minDp,
        preferredDp = preferredDp,
        maxDp = maxDp
    )

    private fun AvoidRegionKindDto.toModel(): AvoidRegionKind = when (this) {
        AvoidRegionKindDto.WINDOW_INSET -> AvoidRegionKind.WindowInset
        AvoidRegionKindDto.VERTICAL_HINGE -> AvoidRegionKind.VerticalHinge
        AvoidRegionKindDto.HORIZONTAL_HINGE -> AvoidRegionKind.HorizontalHinge
    }

    private fun AvoidRegionDto.toModel(): AvoidRegion = AvoidRegion(
        leftDp = leftDp,
        topDp = topDp,
        rightDp = rightDp,
        bottomDp = bottomDp,
        kind = kind.toModel()
    )

    private fun LayoutPlanDto.toModel(): LayoutPlan = LayoutPlan(
        widthClass = widthClass.toModel(),
        heightClass = heightClass.toModel(),
        shellMode = shellMode.toModel(),
        editorMode = editorMode.toModel(),
        navigationMode = navigationMode.toModel(),
        navigationPresentation = navigationPresentation.toModel(),
        workspacePaneMode = workspacePaneMode.toModel(),
        visiblePaneRoles = visiblePaneRoles.toModel(),
        contentMaxWidthDp = contentMaxWidthDp,
        pagePaddingDp = pagePaddingDp,
        gridColumns = gridColumns.toInt(),
        showBottomBar = showBottomBar,
        listPaneWidth = listPaneWidth.toModel(),
        editorContentMaxWidthDp = editorContentMaxWidthDp,
        primaryPaneMinDp = primaryPaneMinDp,
        primaryPanePreferredDp = primaryPanePreferredDp,
        primaryPaneMaxDp = primaryPaneMaxDp,
        supportingPaneMode = supportingPaneMode?.toModel(),
        avoidRegions = avoidRegions.map { it.toModel() }
    )
}
