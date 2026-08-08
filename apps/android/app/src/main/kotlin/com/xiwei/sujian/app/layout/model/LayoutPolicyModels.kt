package com.xiwei.sujian.app.layout.model

enum class FoldState {
    None,
    Flat,
    HalfOpened,
}

enum class FoldOrientation {
    Horizontal,
    Vertical,
}

enum class FoldOcclusion {
    None,
    Full,
}

data class FoldFeatureInfo(
    val state: FoldState = FoldState.None,
    val orientation: FoldOrientation = FoldOrientation.Vertical,
    val isSeparating: Boolean = false,
    val occlusion: FoldOcclusion = FoldOcclusion.None,
    val boundsLeftVp: Float = 0f,
    val boundsTopVp: Float = 0f,
    val boundsRightVp: Float = 0f,
    val boundsBottomVp: Float = 0f,
)

enum class Orientation {
    Unknown,
    Portrait,
    Landscape,
}

enum class PointerKind {
    Unknown,
    Touch,
    Stylus,
    Mouse,
    Trackpad,
}

enum class WidthClass {
    Compact,
    Medium,
    Expanded,
    Large,
    ExtraLarge,
}

enum class HeightClass {
    Compact,
    Medium,
    Expanded,
}

enum class ShellMode {
    SinglePane,
    SupportingPane,
    TwoPane,
    ThreePane,
}

enum class EditorMode {
    FullWidth,
    CenteredPaper,
}

enum class NavigationMode {
    Stack,
    ListDetail,
}

enum class NavigationPresentation {
    BottomBar,
    NavigationRail,
    PermanentDrawer,
}

enum class WorkspacePaneMode {
    SinglePane,
    ListDetail,
    ThreePane,
}

data class VisiblePaneRoles(
    val showProjectList: Boolean = true,
    val showChapterTree: Boolean = true,
    val showEditor: Boolean = true,
    val showSupporting: Boolean = false,
)

data class PaneWidthConstraint(
    val minDp: Float = 0f,
    val preferredDp: Float = 0f,
    val maxDp: Float = 0f,
)

enum class AvoidRegionKind {
    WindowInset,
    VerticalHinge,
    HorizontalHinge,
}

data class AvoidRegion(
    val leftDp: Float = 0f,
    val topDp: Float = 0f,
    val rightDp: Float = 0f,
    val bottomDp: Float = 0f,
    val kind: AvoidRegionKind = AvoidRegionKind.WindowInset,
)

data class WindowMetrics(
    val widthDp: Float,
    val heightDp: Float,
    val safeTopDp: Float = 0f,
    val safeBottomDp: Float = 0f,
    val keyboardVisible: Boolean = false,
    val foldFeature: FoldFeatureInfo = FoldFeatureInfo(),
    val orientation: Orientation = Orientation.Portrait,
    val pointer: PointerKind = PointerKind.Touch,
)

data class LayoutPlan(
    val widthClass: WidthClass,
    val heightClass: HeightClass,
    val shellMode: ShellMode,
    val editorMode: EditorMode,
    val navigationMode: NavigationMode,
    val navigationPresentation: NavigationPresentation,
    val workspacePaneMode: WorkspacePaneMode,
    val visiblePaneRoles: VisiblePaneRoles,
    val contentMaxWidthDp: Float,
    val pagePaddingDp: Float,
    val gridColumns: Int,
    val showBottomBar: Boolean,
    val listPaneWidth: PaneWidthConstraint,
    val editorContentMaxWidthDp: Float,
    val primaryPaneMinDp: Float,
    val primaryPanePreferredDp: Float,
    val primaryPaneMaxDp: Float,
    val supportingPaneMode: WorkspacePaneMode? = null,
    val avoidRegions: List<AvoidRegion> = emptyList(),
)
