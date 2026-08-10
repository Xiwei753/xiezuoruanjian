package com.xiwei.sujian.app.presentation

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.writer_core.ActionRegionDto
import uniffi.writer_core.ActionRoleDto
import uniffi.writer_core.ActionSlotDto
import uniffi.writer_core.ActionTargetDto
import uniffi.writer_core.ScreenPolicyDto
import uniffi.writer_core.ScreenRoleDto

/**
 * #610 评论四、评论六：AndroidWorkspaceActionPolicy 是 Core screen contract 的纯映射层 —
 * 只按 region + target + order 输出可渲染动作 spec，不查业务状态。
 *
 * 测试在 presentation 包，可以用 uniffi DTO 构造输入（ActionSlotDto/ScreenPolicyDto 等），
 * 但断言改用 presentation model（WorkspaceActionKind/Target/Region）— feature UI
 * 不再接触 uniffi DTO 类型。
 *
 * 生产 UI（ProjectListContent / ChapterTreeContent / VolumeRow / ChapterRow /
 * 空态提示）只消费这里的 spec：动作存在性、区域、顺序全部来自 Core 契约，
 * Composable 不再自行决定。
 */
class AndroidWorkspaceActionPolicyTest {
    private fun slot(
        role: ActionRoleDto,
        region: ActionRegionDto,
        order: Int,
        target: ActionTargetDto,
        requiresConfirmation: Boolean = false,
    ): ActionSlotDto =
        ActionSlotDto(
            role = role,
            target = target,
            region = region,
            order = order.toUShort(),
            requiresConfirmation = requiresConfirmation,
        )

    private fun policy(
        screenRole: ScreenRoleDto,
        slots: List<ActionSlotDto>,
    ): ScreenPolicyDto = ScreenPolicyDto(screenRole = screenRole, actionSlots = slots)

    /** 作品列表契约（与 Core resolve_screen_policy(ProjectList) 一致）。 */
    private fun projectListPolicy(): ScreenPolicyDto =
        policy(
            ScreenRoleDto.PROJECT_LIST,
            listOf(
                slot(
                    ActionRoleDto.CREATE_PROJECT,
                    ActionRegionDto.PRIMARY_ACTION,
                    10,
                    ActionTargetDto.PROJECT,
                ),
                slot(
                    ActionRoleDto.DELETE,
                    ActionRegionDto.CONTEXT,
                    10,
                    ActionTargetDto.PROJECT,
                    requiresConfirmation = true,
                ),
                slot(
                    ActionRoleDto.RENAME,
                    ActionRegionDto.CONTEXT,
                    20,
                    ActionTargetDto.PROJECT,
                ),
            ),
        )

    /** 作品工作区契约（与 Core resolve_screen_policy(ProjectWorkspace) 一致）。 */
    private fun projectWorkspacePolicy(): ScreenPolicyDto =
        policy(
            ScreenRoleDto.PROJECT_WORKSPACE,
            listOf(
                slot(ActionRoleDto.SYNC, ActionRegionDto.HEADER_TRAILING, 10, ActionTargetDto.APP),
                slot(ActionRoleDto.SEARCH, ActionRegionDto.HEADER_TRAILING, 20, ActionTargetDto.APP),
                slot(ActionRoleDto.SETTINGS, ActionRegionDto.HEADER_TRAILING, 30, ActionTargetDto.APP),
                slot(ActionRoleDto.CREATE_VOLUME, ActionRegionDto.LIST_HEADER, 10, ActionTargetDto.PROJECT),
                slot(
                    ActionRoleDto.CREATE_CHAPTER,
                    ActionRegionDto.ITEM_TRAILING,
                    10,
                    ActionTargetDto.VOLUME,
                ),
                slot(
                    ActionRoleDto.CREATE_CHAPTER,
                    ActionRegionDto.EMPTY_STATE,
                    10,
                    ActionTargetDto.VOLUME,
                ),
                slot(
                    ActionRoleDto.DELETE,
                    ActionRegionDto.CONTEXT,
                    10,
                    ActionTargetDto.VOLUME,
                    requiresConfirmation = true,
                ),
                slot(
                    ActionRoleDto.DELETE,
                    ActionRegionDto.CONTEXT,
                    20,
                    ActionTargetDto.CHAPTER,
                    requiresConfirmation = true,
                ),
                slot(ActionRoleDto.RENAME, ActionRegionDto.CONTEXT, 30, ActionTargetDto.VOLUME),
                slot(ActionRoleDto.RENAME, ActionRegionDto.CONTEXT, 40, ActionTargetDto.CHAPTER),
                slot(ActionRoleDto.MOVE_EARLIER, ActionRegionDto.CONTEXT, 50, ActionTargetDto.VOLUME),
                slot(ActionRoleDto.MOVE_LATER, ActionRegionDto.CONTEXT, 60, ActionTargetDto.VOLUME),
                slot(ActionRoleDto.MOVE_EARLIER, ActionRegionDto.CONTEXT, 70, ActionTargetDto.CHAPTER),
                slot(ActionRoleDto.MOVE_LATER, ActionRegionDto.CONTEXT, 80, ActionTargetDto.CHAPTER),
            ),
        )

    // ---- 作品列表 ----

    @Test
    fun `project list primary action is create project`() {
        val spec = AndroidWorkspaceActionPolicy.resolve(projectListPolicy())
        assertEquals(1, spec.primaryActions.size)
        val create = spec.primaryActions.single()
        assertEquals(WorkspaceActionKind.CreateProject, create.kind)
        assertEquals(WorkspaceActionTarget.Project, create.target)
        assertEquals(10, create.order)
    }

    @Test
    fun `project list context actions are delete then rename in Core order`() {
        val spec = AndroidWorkspaceActionPolicy.resolve(projectListPolicy())
        val actions = spec.contextActions(WorkspaceActionTarget.Project)
        assertEquals(2, actions.size)
        assertEquals(WorkspaceActionKind.Delete, actions[0].kind)
        assertTrue("Delete 需要确认（契约 requiresConfirmation）", actions[0].requiresConfirmation)
        assertEquals(WorkspaceActionKind.Rename, actions[1].kind)
        // 顺序来自 Core order：Delete(10) → Rename(20)。
        assertTrue(actions[0].order < actions[1].order)
    }

    @Test
    fun `project list has no volume or chapter actions`() {
        val spec = AndroidWorkspaceActionPolicy.resolve(projectListPolicy())
        assertTrue(spec.contextActions(WorkspaceActionTarget.Volume).isEmpty())
        assertTrue(spec.contextActions(WorkspaceActionTarget.Chapter).isEmpty())
        assertTrue(spec.itemTrailingActions(WorkspaceActionTarget.Volume).isEmpty())
        assertTrue(spec.emptyStateActions(WorkspaceActionTarget.Volume).isEmpty())
        assertTrue(spec.listHeaderActions.isEmpty())
    }

    // ---- 作品工作区 ----

    @Test
    fun `workspace list header holds create volume`() {
        val spec = AndroidWorkspaceActionPolicy.resolve(projectWorkspacePolicy())
        assertEquals(1, spec.listHeaderActions.size)
        val createVolume = spec.listHeaderActions.single()
        assertEquals(WorkspaceActionKind.CreateVolume, createVolume.kind)
        assertEquals(WorkspaceActionTarget.Project, createVolume.target)
    }

    @Test
    fun `workspace item trailing holds create chapter for volume target`() {
        val spec = AndroidWorkspaceActionPolicy.resolve(projectWorkspacePolicy())
        val volumeTrailing = spec.itemTrailingActions(WorkspaceActionTarget.Volume)
        assertEquals(1, volumeTrailing.size)
        assertEquals(WorkspaceActionKind.CreateChapter, volumeTrailing.single().kind)
        // 章节目标没有行尾动作。
        assertTrue(spec.itemTrailingActions(WorkspaceActionTarget.Chapter).isEmpty())
    }

    @Test
    fun `workspace context actions distinguish volume and chapter by target`() {
        val spec = AndroidWorkspaceActionPolicy.resolve(projectWorkspacePolicy())
        val volume = spec.contextActions(WorkspaceActionTarget.Volume)
        val chapter = spec.contextActions(WorkspaceActionTarget.Chapter)
        // #610 评论四：卷/章节各有 删除/重命名/上移/下移，靠 target 区分。
        assertEquals(4, volume.size)
        assertEquals(4, chapter.size)
        assertEquals(
            listOf(
                WorkspaceActionKind.Delete,
                WorkspaceActionKind.Rename,
                WorkspaceActionKind.MoveEarlier,
                WorkspaceActionKind.MoveLater,
            ),
            volume.map { it.kind },
        )
        assertEquals(
            listOf(
                WorkspaceActionKind.Delete,
                WorkspaceActionKind.Rename,
                WorkspaceActionKind.MoveEarlier,
                WorkspaceActionKind.MoveLater,
            ),
            chapter.map { it.kind },
        )
        // 顺序来自 Core order（升序），渲染层按此排序菜单项。
        assertTrue(volume.zipWithNext().all { (a, b) -> a.order < b.order })
        assertTrue(chapter.zipWithNext().all { (a, b) -> a.order < b.order })
        // 卷与章节的删除/重命名/顺序动作必须可区分（身份不能靠顺序猜）。
        assertEquals(WorkspaceActionTarget.Volume, volume[0].target)
        assertEquals(WorkspaceActionTarget.Chapter, chapter[0].target)
    }

    @Test
    fun `workspace empty state holds create chapter for volume`() {
        val spec = AndroidWorkspaceActionPolicy.resolve(projectWorkspacePolicy())
        val empty = spec.emptyStateActions(WorkspaceActionTarget.Volume)
        assertEquals(1, empty.size)
        assertEquals(WorkspaceActionKind.CreateChapter, empty.single().kind)
        assertEquals(WorkspaceActionRegion.EmptyState, empty.single().region)
    }

    @Test
    fun `workspace has no primary action`() {
        val spec = AndroidWorkspaceActionPolicy.resolve(projectWorkspacePolicy())
        assertTrue(spec.primaryActions.isEmpty())
    }

    // ---- 负向 ----

    @Test
    fun `null policy yields empty spec`() {
        val spec = AndroidWorkspaceActionPolicy.resolve(null)
        assertTrue(spec.primaryActions.isEmpty())
        assertTrue(spec.listHeaderActions.isEmpty())
        assertTrue(spec.itemTrailingActions(WorkspaceActionTarget.Volume).isEmpty())
        assertTrue(spec.contextActions(WorkspaceActionTarget.Project).isEmpty())
        assertTrue(spec.contextActions(WorkspaceActionTarget.Volume).isEmpty())
        assertTrue(spec.contextActions(WorkspaceActionTarget.Chapter).isEmpty())
        assertTrue(spec.emptyStateActions(WorkspaceActionTarget.Volume).isEmpty())
    }

    @Test
    fun `policy without context slots renders no context actions`() {
        val spec =
            AndroidWorkspaceActionPolicy.resolve(
                policy(
                    ScreenRoleDto.PROJECT_LIST,
                    listOf(
                        slot(
                            ActionRoleDto.CREATE_PROJECT,
                            ActionRegionDto.PRIMARY_ACTION,
                            10,
                            ActionTargetDto.PROJECT,
                        ),
                    ),
                ),
            )
        assertTrue(spec.contextActions(WorkspaceActionTarget.Project).isEmpty())
        // 契约没有的槽位不渲染：PrimaryAction 存在但 Context 为空。
        assertEquals(1, spec.primaryActions.size)
    }

    @Test
    fun `slots are sorted by Core order regardless of input order`() {
        // 输入乱序时，输出必须按 order 升序（渲染层不自己排序）。
        val spec =
            AndroidWorkspaceActionPolicy.resolve(
                policy(
                    ScreenRoleDto.PROJECT_LIST,
                    listOf(
                        slot(
                            ActionRoleDto.RENAME,
                            ActionRegionDto.CONTEXT,
                            20,
                            ActionTargetDto.PROJECT,
                        ),
                        slot(
                            ActionRoleDto.DELETE,
                            ActionRegionDto.CONTEXT,
                            10,
                            ActionTargetDto.PROJECT,
                            requiresConfirmation = true,
                        ),
                    ),
                ),
            )
        val actions = spec.contextActions(WorkspaceActionTarget.Project)
        assertEquals(listOf(WorkspaceActionKind.Delete, WorkspaceActionKind.Rename), actions.map { it.kind })
    }
}
