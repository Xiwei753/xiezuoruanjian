package com.xiwei.sujian.app.presentation

import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import uniffi.writer_core.ActionRegionDto
import uniffi.writer_core.ActionRoleDto
import uniffi.writer_core.ActionSlotDto
import uniffi.writer_core.ActionTargetDto
import uniffi.writer_core.ScreenPolicyDto

/**
 * Android Workspace Action Policy（#610 评论四）— 把 Core 的 `ScreenPolicyDto`
 * 变成 Android 可渲染的 workspace 动作 spec。
 *
 * 职责边界：
 * - 输入只接 [ScreenPolicyDto]（Core screen contract，唯一事实来源）；
 * - 输出按 Core `region + target + order` 排好的动作描述：
 *   `primaryActions / listHeaderActions / itemTrailingActions(target) /
 *   contextActions(target) / emptyStateActions(target)`；
 * - 不调用 ProjectViewModel、不做任何业务，只做“契约 → 可渲染动作 spec”的纯映射；
 * - 动作是否存在、出现在哪个产品区域、排第几个，全部由 Core 决定，
 *   本层和 feature UI 不再自行发明。
 *
 * 渲染层（ProjectListScreen / VolumeChapterTree）只按 spec 画按钮/菜单，
 * 点击后再把 `role + target` 绑定到现有业务回调。
 */
internal data class WorkspaceActionSpec(
    val role: ActionRoleDto,
    val target: ActionTargetDto,
    val region: ActionRegionDto,
    /** 同一区域内从左到右（或从主到次）的显示顺序（Core 产品语义）。 */
    val order: Int,
    val requiresConfirmation: Boolean,
) {
    companion object {
        fun from(slot: ActionSlotDto): WorkspaceActionSpec =
            WorkspaceActionSpec(
                role = slot.role,
                target = slot.target,
                region = slot.region,
                order = slot.order.toInt(),
                requiresConfirmation = slot.requiresConfirmation,
            )
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
        ordered.filter { it.region == ActionRegionDto.PRIMARY_ACTION }

    /** ListHeader 区域（新建卷等），按 order 升序。 */
    val listHeaderActions: List<WorkspaceActionSpec> =
        ordered.filter { it.region == ActionRegionDto.LIST_HEADER }

    /** ItemTrailing 区域中作用于 [target] 的动作（新建章节），按 order 升序。 */
    fun itemTrailingActions(target: ActionTargetDto): List<WorkspaceActionSpec> =
        ordered.filter { it.region == ActionRegionDto.ITEM_TRAILING && it.target == target }

    /** Context 区域中作用于 [target] 的动作（删除/重命名/上移/下移），按 order 升序。 */
    fun contextActions(target: ActionTargetDto): List<WorkspaceActionSpec> =
        ordered.filter { it.region == ActionRegionDto.CONTEXT && it.target == target }

    /** EmptyState 区域中作用于 [target] 的动作（空卷新建章节），按 order 升序。 */
    fun emptyStateActions(target: ActionTargetDto): List<WorkspaceActionSpec> =
        ordered.filter { it.region == ActionRegionDto.EMPTY_STATE && it.target == target }

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
     */
    fun resolve(policy: ScreenPolicyDto?): AndroidWorkspaceActionSpec {
        val slots = policy?.actionSlots.orEmpty().map { WorkspaceActionSpec.from(it) }
        return AndroidWorkspaceActionSpec(slots)
    }
}

/**
 * #610 评论四：在 Composable 中从 Core ScreenPolicyDto 解析 workspace 动作 spec。
 *
 * 放在 presentation 层，navigation/feature UI 无需接触 uniffi DTO 类型。
 */
@Composable
internal fun rememberWorkspaceActions(screenPolicy: ScreenPolicyDto?): AndroidWorkspaceActionSpec =
    remember(screenPolicy) { AndroidWorkspaceActionPolicy.resolve(screenPolicy) }
