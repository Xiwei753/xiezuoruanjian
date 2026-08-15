package com.xiwei.sujian.app.presentation.screen

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import uniffi.writer_core.ActionRegionDto
import uniffi.writer_core.ActionRoleDto
import uniffi.writer_core.ActionSlotDto
import uniffi.writer_core.ActionTargetDto
import uniffi.writer_core.ScreenPolicyDto
import uniffi.writer_core.ScreenRoleDto

/*
 * Android Workspace Action Policy（#610 评论四、评论六）— 把 Core 的 `ScreenPolicyDto`
 * 变成 Android 可渲染的 workspace 动作 spec。
 *
 * 职责边界：
 * - 输入只接 [ScreenPolicyDto]（Core screen contract，唯一事实来源）；
 * - 输出按 Core `region + target + order` 排好的动作描述：
 *   `primaryActions / listHeaderActions / itemTrailingActions(target) /
 *   contextActions(target) / emptyStateActions(target)`；
 * - 不调用 ProjectViewModel、不做任何业务，只做"契约 → 可渲染动作 spec"的纯映射；
 * - 动作是否存在、出现在哪个产品区域、排第几个，全部由 Core 决定，
 *   本层和 feature UI 不再自行发明。
 *
 * #610 评论六：presentation 层不再把 UniFFI DTO（ActionRoleDto/ActionTargetDto/
 * ActionRegionDto）暴露给 feature UI。feature UI 只能依赖本文件定义的
 * [WorkspaceActionKind]/[WorkspaceActionTarget]/[WorkspaceActionRegion]。
 * [WorkspaceActionSpec.from] 是唯一允许知道 UniFFI presentation DTO 的
 * Android presentation 边界。
 *
 * 渲染层（ProjectListScreen / VolumeChapterTree）只按 spec 画按钮/菜单，
 * 点击后再把 `kind + target` 绑定到现有业务回调。
 */

/**
 * Workspace 动作种类（Android presentation 自己的枚举，#610 评论六）。
 *
 * 只覆盖 workspace 动作（CreateProject/CreateVolume/CreateChapter/Delete/
 * Rename/MoveEarlier/MoveLater）。header 动作（Sync/Search/Settings/Back）
 * 由 AndroidChromePolicy 处理，不属于本枚举。
 */
internal enum class WorkspaceActionKind {
    CreateProject,
    CreateVolume,
    CreateChapter,
    Delete,
    Rename,
    MoveEarlier,
    MoveLater,
}

/** Workspace 动作目标（Android presentation 自己的枚举，#610 评论六）。 */
internal enum class WorkspaceActionTarget {
    App,
    Project,
    Volume,
    Chapter,
}

/**
 * Workspace 动作区域（Android presentation 自己的枚举，#610 评论六）。
 *
 * 只覆盖 workspace 区域。header 区域（HeaderLeading/HeaderTrailing）由
 * AndroidChromePolicy 处理，不属于本枚举。
 */
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
    /** 同一区域内从左到右（或从主到次）的显示顺序（Core 产品语义）。 */
    val order: Int,
    val requiresConfirmation: Boolean,
) {
    companion object {
        /**
         * Core `ActionSlotDto` → Android presentation [WorkspaceActionSpec]。
         *
         * #610 评论六：这是唯一允许知道 UniFFI presentation DTO 的 Android presentation 边界。
         *
         * 返回 `null` 的两种情况：
         * - 非 workspace 动作（Sync/Search/Settings/Back，由 AndroidChromePolicy 处理）；
         * - 非 workspace 区域（HeaderLeading/HeaderTrailing）。
         */
        fun from(slot: ActionSlotDto): WorkspaceActionSpec? {
            val kind = slot.role.toWorkspaceActionKind() ?: return null
            val target = slot.target.toWorkspaceActionTarget()
            val region = slot.region.toWorkspaceActionRegion() ?: return null
            return WorkspaceActionSpec(
                kind = kind,
                target = target,
                region = region,
                order = slot.order.toInt(),
                requiresConfirmation = slot.requiresConfirmation,
            )
        }

        private fun ActionRoleDto.toWorkspaceActionKind(): WorkspaceActionKind? =
            when (this) {
                ActionRoleDto.CREATE_PROJECT -> WorkspaceActionKind.CreateProject
                ActionRoleDto.CREATE_VOLUME -> WorkspaceActionKind.CreateVolume
                ActionRoleDto.CREATE_CHAPTER -> WorkspaceActionKind.CreateChapter
                ActionRoleDto.DELETE -> WorkspaceActionKind.Delete
                ActionRoleDto.RENAME -> WorkspaceActionKind.Rename
                ActionRoleDto.MOVE_EARLIER -> WorkspaceActionKind.MoveEarlier
                ActionRoleDto.MOVE_LATER -> WorkspaceActionKind.MoveLater
                // Sync/Search/Settings/Back 属于 header 动作，由 AndroidChromePolicy 处理。
                else -> null
            }

        private fun ActionTargetDto.toWorkspaceActionTarget(): WorkspaceActionTarget =
            when (this) {
                ActionTargetDto.APP -> WorkspaceActionTarget.App
                ActionTargetDto.PROJECT -> WorkspaceActionTarget.Project
                ActionTargetDto.VOLUME -> WorkspaceActionTarget.Volume
                ActionTargetDto.CHAPTER -> WorkspaceActionTarget.Chapter
            }

        private fun ActionRegionDto.toWorkspaceActionRegion(): WorkspaceActionRegion? =
            when (this) {
                ActionRegionDto.PRIMARY_ACTION -> WorkspaceActionRegion.PrimaryAction
                ActionRegionDto.LIST_HEADER -> WorkspaceActionRegion.ListHeader
                ActionRegionDto.ITEM_TRAILING -> WorkspaceActionRegion.ItemTrailing
                ActionRegionDto.CONTEXT -> WorkspaceActionRegion.Context
                ActionRegionDto.EMPTY_STATE -> WorkspaceActionRegion.EmptyState
                // HeaderLeading/HeaderTrailing 属于 header 区域，由 AndroidChromePolicy 处理。
                else -> null
            }
    }
}

/**
 * 按 Core region 分组、order 升序排好的动作 spec。
 *
 * 各区域列表的排序与分组完全来自 Core 契约；渲染层只消费这里的结果。
 */
internal class AndroidWorkspaceActionSpec(slots: List<WorkspaceActionSpec>) {
    private val ordered = slots.sortedBy { it.order }

    /** PrimaryAction 区域（新建作品等页面主操作），按 order 升序。 */
    val primaryActions: List<WorkspaceActionSpec> =
        ordered.filter { it.region == WorkspaceActionRegion.PrimaryAction }

    /** ListHeader 区域（新建卷等），按 order 升序。 */
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
        /** 契约缺失（桥失败/空契约）时的安全空 spec — 渲染层不画任何动作。 */
        val EMPTY: AndroidWorkspaceActionSpec = AndroidWorkspaceActionSpec(emptyList())
    }
}

internal object AndroidWorkspaceActionPolicy {
    /**
     * ScreenPolicyDto → 可渲染 workspace 动作 spec。
     *
     * 纯映射：不查业务状态、不访问 ViewModel/Repository，
     * 只按 Core 的 region/target/order 分组排序。
     *
     * #610 评论六：用 `mapNotNull` 跳过非 workspace 动作（Sync/Search/Settings/Back）
     * 和非 workspace 区域（HeaderLeading/HeaderTrailing），它们由 AndroidChromePolicy 处理。
     *
     * #618 一：调用方固定从 [PresentationPolicyCatalog] 取对应角色的 ScreenPolicyDto
     * 再 resolve — 动作契约不再依赖调用时刻的导航/组合帧。
     */
    fun resolve(policy: ScreenPolicyDto?): AndroidWorkspaceActionSpec {
        val slots = policy?.actionSlots.orEmpty().mapNotNull { WorkspaceActionSpec.from(it) }
        return AndroidWorkspaceActionSpec(slots)
    }
}

/**
 * #618 一：作品工作区的两份静态动作 spec — 章节树按 PROJECT_WORKSPACE、作品列表按
 * PROJECT_LIST，都来自容器创建时解析的 [PresentationPolicyCatalog]，随 catalog 稳定保存。
 * 页面组合不再按当前导航位置临时解析动作契约。
 *
 * 放在 presentation 层（本层允许 uniffi contract DTO），navigation/feature UI
 * 无需接触 uniffi 类型。
 */
@Composable
internal fun rememberProjectActions(
    catalog: PresentationPolicyCatalog,
): Pair<AndroidWorkspaceActionSpec, AndroidWorkspaceActionSpec> =
    remember(catalog) {
        AndroidWorkspaceActionPolicy.resolve(catalog[ScreenRoleDto.PROJECT_LIST]) to
            AndroidWorkspaceActionPolicy.resolve(catalog[ScreenRoleDto.PROJECT_WORKSPACE])
    }
