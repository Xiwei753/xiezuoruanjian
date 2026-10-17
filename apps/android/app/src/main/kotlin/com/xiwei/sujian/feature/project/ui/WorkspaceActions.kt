package com.xiwei.sujian.feature.project.ui

/*
 * 工作区动作模型 — Android 本地产品语义。
 *
 * 布局与控件归属已由各平台 UI 层自行决定，Core 不再返回页面动作槽位契约。
 * 这里用纯 Kotlin 常量描述 Android 工作区（作品列表 / 章节树）真实存在的动作：
 * 动作种类、业务目标、所属产品区域与显示顺序都集中在 [WorkspaceActions] 的静态样本里，
 * 页面组合热路径只做查表，不在每个 Composable 里散落"哪些动作在哪个区域"的第二套规则。
 *
 * 渲染方（[ProjectListContent] / [ChapterTreeContent] / VolumeRow / ChapterRow / 空态）
 * 只消费这里的分组结果；点击后再绑定到既有业务回调。
 */

/** 动作种类 — 只覆盖工作区动作。页头动作（同步/搜索/设置/返回）由导航层顶栏处理。 */
internal enum class WorkspaceActionKind {
    CreateProject,
    CreateVolume,
    CreateChapter,
    Delete,
    Rename,
    MoveEarlier,
    MoveLater,
}

/** 动作的业务目标 — 平台无关身份，用于区分"删卷/删章节"等同名动作。 */
internal enum class WorkspaceActionTarget {
    App,
    Project,
    Volume,
    Chapter,
}

/** 动作所属产品区域 — 只覆盖工作区区域。 */
internal enum class WorkspaceActionRegion {
    PrimaryAction,
    ListHeader,
    ItemTrailing,
    Context,
    EmptyState,
}

internal data class WorkspaceActionSpec(
    val kind: WorkspaceActionKind,
    val target: WorkspaceActionTarget,
    val region: WorkspaceActionRegion,
    /** 同一区域内从左到右（或从主到次）的显示顺序。 */
    val order: Int,
    val requiresConfirmation: Boolean,
)

/**
 * 一组按 [WorkspaceActionSpec.order] 排好的工作区动作。
 *
 * 各区域列表的排序与分组只在此处完成；渲染层只消费结果。
 */
internal class WorkspaceActions(specs: List<WorkspaceActionSpec>) {
    private val ordered = specs.sortedBy { it.order }

    /** PrimaryAction 区域（新建作品/新建卷等页面主操作），按 order 升序。 */
    val primaryActions: List<WorkspaceActionSpec> =
        ordered.filter { it.region == WorkspaceActionRegion.PrimaryAction }

    /** ListHeader 区域，按 order 升序。 */
    val listHeaderActions: List<WorkspaceActionSpec> =
        ordered.filter { it.region == WorkspaceActionRegion.ListHeader }

    /** ItemTrailing 区域中作用于 [target] 的动作（新建章节），按 order 升序。 */
    fun itemTrailingActions(target: WorkspaceActionTarget): List<WorkspaceActionSpec> =
        ordered.filter { it.region == WorkspaceActionRegion.ItemTrailing && it.target == target }

    /** Context 区域中作用于 [target] 的动作（删除/重命名/上移/下移），按 order 升序。 */
    fun contextActions(target: WorkspaceActionTarget): List<WorkspaceActionSpec> =
        ordered.filter { it.region == WorkspaceActionRegion.Context && it.target == target }

    /** EmptyState 区域中作用于 [target] 的动作（空卷新建章节），按 order 升序。 */
    fun emptyStateActions(target: WorkspaceActionTarget): List<WorkspaceActionSpec> =
        ordered.filter { it.region == WorkspaceActionRegion.EmptyState && it.target == target }

    companion object {
        val EMPTY: WorkspaceActions = WorkspaceActions(emptyList())

        /**
         * 作品列表动作 — 新建作品主操作 + 作品上下文菜单（删除/重命名）。
         * 顺序与旧 Core 契约一致：Context 升序为 删除(10) → 重命名(20)。
         */
        val ProjectList: WorkspaceActions =
            WorkspaceActions(
                listOf(
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.CreateProject,
                        target = WorkspaceActionTarget.Project,
                        region = WorkspaceActionRegion.PrimaryAction,
                        order = 10,
                        requiresConfirmation = false,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.Delete,
                        target = WorkspaceActionTarget.Project,
                        region = WorkspaceActionRegion.Context,
                        order = 10,
                        requiresConfirmation = true,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.Rename,
                        target = WorkspaceActionTarget.Project,
                        region = WorkspaceActionRegion.Context,
                        order = 20,
                        requiresConfirmation = false,
                    ),
                ),
            )

        /**
         * 章节树动作 — 新建卷主操作 + 卷/章节的创建、删除、重命名、排序。
         * 顺序与旧 Core 契约一致（卷与章节用 [WorkspaceActionTarget] 区分身份）。
         */
        val ProjectWorkspace: WorkspaceActions =
            WorkspaceActions(
                listOf(
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.CreateVolume,
                        target = WorkspaceActionTarget.Project,
                        region = WorkspaceActionRegion.PrimaryAction,
                        order = 10,
                        requiresConfirmation = false,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.CreateChapter,
                        target = WorkspaceActionTarget.Volume,
                        region = WorkspaceActionRegion.ItemTrailing,
                        order = 10,
                        requiresConfirmation = false,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.CreateChapter,
                        target = WorkspaceActionTarget.Volume,
                        region = WorkspaceActionRegion.EmptyState,
                        order = 10,
                        requiresConfirmation = false,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.Delete,
                        target = WorkspaceActionTarget.Volume,
                        region = WorkspaceActionRegion.Context,
                        order = 10,
                        requiresConfirmation = true,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.Delete,
                        target = WorkspaceActionTarget.Chapter,
                        region = WorkspaceActionRegion.Context,
                        order = 20,
                        requiresConfirmation = true,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.Rename,
                        target = WorkspaceActionTarget.Volume,
                        region = WorkspaceActionRegion.Context,
                        order = 30,
                        requiresConfirmation = false,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.Rename,
                        target = WorkspaceActionTarget.Chapter,
                        region = WorkspaceActionRegion.Context,
                        order = 40,
                        requiresConfirmation = false,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.MoveEarlier,
                        target = WorkspaceActionTarget.Volume,
                        region = WorkspaceActionRegion.Context,
                        order = 50,
                        requiresConfirmation = false,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.MoveLater,
                        target = WorkspaceActionTarget.Volume,
                        region = WorkspaceActionRegion.Context,
                        order = 60,
                        requiresConfirmation = false,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.MoveEarlier,
                        target = WorkspaceActionTarget.Chapter,
                        region = WorkspaceActionRegion.Context,
                        order = 70,
                        requiresConfirmation = false,
                    ),
                    WorkspaceActionSpec(
                        kind = WorkspaceActionKind.MoveLater,
                        target = WorkspaceActionTarget.Chapter,
                        region = WorkspaceActionRegion.Context,
                        order = 80,
                        requiresConfirmation = false,
                    ),
                ),
            )
    }
}
