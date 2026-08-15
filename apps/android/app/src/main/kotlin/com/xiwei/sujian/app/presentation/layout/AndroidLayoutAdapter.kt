package com.xiwei.sujian.app.presentation.layout

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import com.xiwei.sujian.core.platform.window.AospFoldFeatureInfo
import uniffi.writer_core.LayoutContractDto
import uniffi.writer_core.PrimaryNavigationPlacementDto
import uniffi.writer_core.WindowOcclusionDto
import uniffi.writer_core.WindowViewportDto
import uniffi.writer_core.WorkbenchLayoutPlanDto
import uniffi.writer_core.WorkbenchRoleDto
import uniffi.writer_core.WorkbenchVisibilityDto
import uniffi.writer_core.WorkspaceLayoutModeDto

/**
 * Android Layout Adapter（#610 / #628）— Android presentation/layout 层。
 *
 * 职责只剩两件事（#628 评论第 2 节）：
 * 1. 用 [LocalWindowInfo] 读取当前 Compose 宿主窗口的原始 dp 宽高 + 折叠铰链 occlusion，
 *    构造 [WindowViewportDto] 交给 Core `presentation/layout` 解析；
 * 2. 把 Rust [LayoutContractDto] / [WorkbenchLayoutPlanDto] 映射成具体控件
 *    （[AndroidLayoutSpec] / [AndroidWorkbenchLayoutPlan]）。
 *
 * #628 评论 5301021120 第 3-4 步：新增 [rememberWorkbenchLayoutPlan] —
 * 用当前 viewport + visibility 请求 Rust workbench plan，按 plan 放 slot。
 * Android 只做 dp→px 和 place，不再判断 hinge 在左还是右、不决定角色挪到哪一侧。
 *
 * 调用链（#628 评论第 4 节）：
 * ```text
 * Android LocalWindowInfo.containerDpSize + AospFoldFeatureInfo.bounds
 *         ↓
 * WindowViewportDto(width, height, occlusions)
 *         ↓
 * Rust presentation/layout (breakpoints/metrics/resolver)
 *         ↓
 * LayoutContractDto / WorkbenchLayoutPlanDto
 *         ↓
 * AndroidLayoutAdapter
 *         ↓
 * AndroidLayoutSpec / AndroidWorkbenchLayoutPlan (contract + 便捷视图)
 * ```
 */
@Composable
internal fun rememberAndroidLayoutSpec(
    foldingFeatures: List<AospFoldFeatureInfo>,
    resolveLayoutContract: (WindowViewportDto) -> LayoutContractDto?,
): AndroidLayoutSpec {
    // #628 验收点 3：窗口尺寸改用 LocalWindowInfo.current.containerDpSize（DpSize）。
    // 取 width.value / height.value 得到 Float dp，构造 WindowViewportDto。
    val windowInfo = LocalWindowInfo.current
    val containerDpSize = windowInfo.containerDpSize

    // #628 验收点 5：折叠铰链 → WindowOcclusionDto。
    // AospFoldFeatureInfo.bounds 是 px 坐标，用 LocalDensity 转 dp。
    val density = LocalDensity.current
    val occlusions =
        remember(foldingFeatures, density) {
            foldingFeatures
                .filter { it.isSeparating }
                .map { feature ->
                    with(density) {
                        WindowOcclusionDto(
                            leftDp = feature.boundsLeft.toDp().value,
                            topDp = feature.boundsTop.toDp().value,
                            rightDp = feature.boundsRight.toDp().value,
                            bottomDp = feature.boundsBottom.toDp().value,
                            separating = feature.isSeparating,
                        )
                    }
                }
        }

    val viewport =
        remember(containerDpSize, occlusions) {
            WindowViewportDto(
                widthDp = containerDpSize.width.value,
                heightDp = containerDpSize.height.value,
                occlusions = occlusions,
            )
        }
    val contract =
        remember(viewport) {
            resolveLayoutContract(viewport)
        }

    return AndroidLayoutSpec(contract = contract, viewport = viewport)
}

/**
 * #628 评论 5301021120 第 4 步：用当前 viewport + visibility 请求 Rust workbench plan。
 *
 * 只在窗口几何（[viewport]）或 visibility 变化时重算，不是每帧请求。
 * 返回 [AndroidWorkbenchLayoutPlan]（Kotlin 侧纯数据，避免 UI 层直接引用 uniffi DTO）。
 */
@Composable
internal fun rememberWorkbenchLayoutPlan(
    viewport: WindowViewportDto,
    visibility: AndroidWorkbenchVisibility,
    resolveWorkbenchLayout: (WindowViewportDto, WorkbenchVisibilityDto) -> WorkbenchLayoutPlanDto?,
): AndroidWorkbenchLayoutPlan {
    val visibilityDto =
        remember(visibility) {
            WorkbenchVisibilityDto(
                chapterNavigationVisible = visibility.chapterNavigationVisible,
                toolPaneVisible = visibility.toolPaneVisible,
            )
        }
    val planDto =
        remember(viewport, visibilityDto) {
            resolveWorkbenchLayout(viewport, visibilityDto)
        }
    return remember(planDto) { planDto?.toAndroidWorkbenchLayoutPlan() ?: AndroidWorkbenchLayoutPlan(emptyList()) }
}

/**
 * #628 评论 5301021120 第 4 步：用默认 visibility（都可见）请求 Rust workbench plan。
 *   chapterTreeCollapsed/toolPaneCollapsed 是 WideWritingWorkspace 内的局部 UI 状态；
 *   WideWritingWorkspace 内部收起状态变化时仍按 plan 放 slot（收起通过不画对应 slot 实现）。
 *   完整的 per-visibility 重算在 WideWritingWorkspace 内部按需触发（#628 后续优化点）。
 *
 * 放在 presentation/layout 层以避免 UI 层（app/navigation）直接引用 uniffi DTO（架构门禁）。
 */
@Composable
internal fun rememberDefaultWorkbenchLayoutPlan(
    viewport: WindowViewportDto,
    workbenchResolver: (WindowViewportDto, WorkbenchVisibilityDto) -> WorkbenchLayoutPlanDto?,
): AndroidWorkbenchLayoutPlan =
    rememberWorkbenchLayoutPlan(
        viewport = viewport,
        visibility = AndroidWorkbenchVisibility(chapterNavigationVisible = true, toolPaneVisible = true),
        resolveWorkbenchLayout = workbenchResolver,
    )

/**
 * #625 第二段 / #628 验收点 1：工作区布局模式 — Kotlin 侧枚举，
 * 避免 UI 层直接引用 uniffi DTO（遵守 ui-no-uniffi-jna-bridge 架构门禁）。
 *
 * 由 Core `LayoutContractDto.workspaceLayoutMode` 决定（#628：窗口尺寸→布局决策唯一在 Rust）。
 * - [WorkspaceLayoutMode.SINGLE_PANE]：窄屏单栏；
 * - [WorkspaceLayoutMode.WORKBENCH]：大屏工作台（左章节树 + 中央编辑器 + 右工具面板）。
 */
internal enum class WorkspaceLayoutMode {
    SINGLE_PANE,
    WORKBENCH,
}

/** Core [WorkspaceLayoutModeDto] → Kotlin [WorkspaceLayoutMode]（interop 映射，非断点判断）。 */
internal fun WorkspaceLayoutModeDto.toWorkspaceLayoutMode(): WorkspaceLayoutMode =
    when (this) {
        WorkspaceLayoutModeDto.SINGLE_PANE -> WorkspaceLayoutMode.SINGLE_PANE
        WorkspaceLayoutModeDto.WORKBENCH -> WorkspaceLayoutMode.WORKBENCH
    }

/**
 * Android UI spec — AndroidLayoutAdapter 的最终输出。
 *
 * [contract] 是 Core presentation contract（产品壳层语义，含一级导航放置、共用尺寸、
 * 工作区布局模式）。
 * [viewport] 是当前窗口视口（dp），供 [rememberWorkbenchLayoutPlan] 复用，
 * 避免重复测量。
 *
 * #628 评论 5301021120 第 1 步：删除 `workbenchOcclusion` 字段（死数据）。
 * 工作台布局计划改由 [rememberWorkbenchLayoutPlan] 单独提供。
 *
 * #628 验收点 2：删除 `scaffoldDirective` 字段 — Material3 PaneScaffoldDirective 整条死链
 * 已删除（无消费者），断点/壳层/导航放置全由 Rust 决定。
 */
internal data class AndroidLayoutSpec(
    val contract: LayoutContractDto?,
    val viewport: WindowViewportDto,
) {
    /**
     * 一级导航是否用底栏（NavigationBar）而非侧栏（NavigationRail）。
     *
     * 由 Core `LayoutContractDto.primaryNavigationPlacement` 决定（#628 评论第 4 节）：
     * - `Bottom` → true（手机/小平板）；
     * - `Side` → false（桌面/大平板）；
     * - 契约缺失（桥失败/空契约）→ true（默认底栏，与窄窗口基线一致）。
     */
    val useBottomNavigation: Boolean
        get() = contract?.primaryNavigationPlacement == PrimaryNavigationPlacementDto.BOTTOM

    /**
     * 工作区布局模式（#625 第二段 / #628 验收点 1）— 供 feature/ui 层判断窄屏/大屏布局。
     *
     * 由 Core `LayoutContractDto.workspaceLayoutMode` 决定（#628：窗口尺寸→布局决策唯一在 Rust）。
     * 契约缺失（桥失败/空契约）→ [WorkspaceLayoutMode.SINGLE_PANE]（默认窄屏，与基线一致）。
     */
    val workspaceLayoutMode: WorkspaceLayoutMode
        get() = contract?.workspaceLayoutMode?.toWorkspaceLayoutMode() ?: WorkspaceLayoutMode.SINGLE_PANE
}

// ── Workbench Layout Plan Kotlin 侧纯数据（#628 评论 5301021120 第 3-4 步） ──

/** 工作台角色 — Kotlin 侧枚举（避免 UI 层直接引用 uniffi DTO）。 */
internal enum class AndroidWorkbenchRole {
    TOOLBAR_LEADING,
    TOOLBAR_CENTER,
    TOOLBAR_TRAILING,
    CHAPTER_NAVIGATION,
    EDITOR,
    TOOL_PANE,
    TOOL_RAIL,
}

/** Core [WorkbenchRoleDto] → Kotlin [AndroidWorkbenchRole]。 */
internal fun WorkbenchRoleDto.toAndroidWorkbenchRole(): AndroidWorkbenchRole =
    when (this) {
        WorkbenchRoleDto.TOOLBAR_LEADING -> AndroidWorkbenchRole.TOOLBAR_LEADING
        WorkbenchRoleDto.TOOLBAR_CENTER -> AndroidWorkbenchRole.TOOLBAR_CENTER
        WorkbenchRoleDto.TOOLBAR_TRAILING -> AndroidWorkbenchRole.TOOLBAR_TRAILING
        WorkbenchRoleDto.CHAPTER_NAVIGATION -> AndroidWorkbenchRole.CHAPTER_NAVIGATION
        WorkbenchRoleDto.EDITOR -> AndroidWorkbenchRole.EDITOR
        WorkbenchRoleDto.TOOL_PANE -> AndroidWorkbenchRole.TOOL_PANE
        WorkbenchRoleDto.TOOL_RAIL -> AndroidWorkbenchRole.TOOL_RAIL
    }

/** 平台无关的布局矩形（dp 坐标系）— Kotlin 侧纯数据。 */
internal data class AndroidLayoutRect(
    val leftDp: Float,
    val topDp: Float,
    val rightDp: Float,
    val bottomDp: Float,
) {
    val widthDp: Float get() = (rightDp - leftDp).coerceAtLeast(0f)
    val heightDp: Float get() = (bottomDp - topDp).coerceAtLeast(0f)
    val isEmpty: Boolean get() = widthDp <= 0f || heightDp <= 0f
}

/** 单个角色的放置 — 角色与其最终 bounds（dp）。 */
internal data class AndroidWorkbenchPlacement(
    val role: AndroidWorkbenchRole,
    val bounds: AndroidLayoutRect,
)

/** 工作台可见性 — 端侧局部 UI 状态（#628 评论 5301021120 第 1 步）。 */
internal data class AndroidWorkbenchVisibility(
    val chapterNavigationVisible: Boolean,
    val toolPaneVisible: Boolean,
)

/** 工作台布局计划 — Kotlin 侧纯数据，含七角色 placement。 */
internal data class AndroidWorkbenchLayoutPlan(
    val placements: List<AndroidWorkbenchPlacement>,
) {
    /** 取指定角色的 placement；不存在时返回 null。 */
    fun placementFor(role: AndroidWorkbenchRole): AndroidWorkbenchPlacement? =
        placements.firstOrNull { it.role == role }
}

/** Core [WorkbenchLayoutPlanDto] → Kotlin [AndroidWorkbenchLayoutPlan]（interop 映射）。 */
internal fun WorkbenchLayoutPlanDto.toAndroidWorkbenchLayoutPlan(): AndroidWorkbenchLayoutPlan =
    AndroidWorkbenchLayoutPlan(
        placements =
            placements.map { p ->
                AndroidWorkbenchPlacement(
                    role = p.role.toAndroidWorkbenchRole(),
                    bounds =
                        AndroidLayoutRect(
                            leftDp = p.bounds.leftDp,
                            topDp = p.bounds.topDp,
                            rightDp = p.bounds.rightDp,
                            bottomDp = p.bounds.bottomDp,
                        ),
                )
            },
    )
