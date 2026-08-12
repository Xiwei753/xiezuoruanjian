package com.xiwei.sujian.app.presentation

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.writer_core.ActionRegionDto
import uniffi.writer_core.ActionRoleDto
import uniffi.writer_core.ActionSlotDto
import uniffi.writer_core.ActionTargetDto
import uniffi.writer_core.ScreenPolicyDto
import uniffi.writer_core.ScreenRoleDto

/**
 * #618 一：PresentationPolicyCatalog 在容器创建时一次性解析静态页面契约，
 * Compose 热路径只查内存 Map。
 *
 * 契约内容（哪些动作在哪些区域）的唯一事实来源是 Core screen_contract 及其
 * Rust 单测（screen_contract_tests.rs）；本测试通过注入 resolver 验证目录自身的
 * 行为：
 * - 六个 Android 消费的角色在构造时全部解析一次；
 * - 解析失败的角色返回 null，不崩溃；
 * - 目录是静态快照：同一实例重复查询返回同一份契约实例（引用相等）；
 * - 未预解析的角色（如 SYNC）不解析、返回 null；
 * - PROJECT_WORKSPACE 契约的卷章创建动作完整交给消费方
 *   （AndroidWorkspaceActionPolicy.resolve 能看到 ListHeader/ItemTrailing/EmptyState 三入口）。
 */
class PresentationPolicyCatalogTest {
    private fun slot(
        role: ActionRoleDto,
        region: ActionRegionDto,
        order: Int,
        target: ActionTargetDto,
    ): ActionSlotDto =
        ActionSlotDto(
            role = role,
            target = target,
            region = region,
            order = order.toUShort(),
            requiresConfirmation = false,
        )

    private fun policy(
        screenRole: ScreenRoleDto,
        slots: List<ActionSlotDto>,
    ): ScreenPolicyDto = ScreenPolicyDto(screenRole = screenRole, actionSlots = slots)

    /** 与 Core ProjectWorkspace 契约一致的测试样本（真实性由 Core 单测保证）。 */
    private fun projectWorkspacePolicy(): ScreenPolicyDto =
        policy(
            ScreenRoleDto.PROJECT_WORKSPACE,
            listOf(
                slot(ActionRoleDto.CREATE_VOLUME, ActionRegionDto.LIST_HEADER, 10, ActionTargetDto.PROJECT),
                slot(ActionRoleDto.CREATE_CHAPTER, ActionRegionDto.ITEM_TRAILING, 10, ActionTargetDto.VOLUME),
                slot(ActionRoleDto.CREATE_CHAPTER, ActionRegionDto.EMPTY_STATE, 10, ActionTargetDto.VOLUME),
            ),
        )

    @Test
    fun constructor_resolvesAllAndroidRolesOnce() {
        val queried = mutableListOf<ScreenRoleDto>()
        val catalog =
            PresentationPolicyCatalog { role ->
                queried.add(role)
                ScreenPolicyDto(screenRole = role, actionSlots = emptyList())
            }
        assertEquals(PresentationPolicyCatalog.ROLES, queried)
        assertEquals(6, queried.size)
    }

    @Test
    fun failedRoleResolution_returnsNull_withoutCrashingOthers() {
        var calls = 0
        val catalog =
            PresentationPolicyCatalog { role ->
                calls++
                if (role == ScreenRoleDto.STAR_MAP) {
                    null
                } else {
                    ScreenPolicyDto(
                        screenRole = role,
                        actionSlots = emptyList(),
                    )
                }
            }
        assertNull(catalog[ScreenRoleDto.STAR_MAP])
        assertTrue(catalog[ScreenRoleDto.SETTINGS] != null)
        assertEquals(6, calls)
    }

    @Test
    fun catalog_isStaticSnapshot_repeatedQueriesReturnSameInstance() {
        val workspacePolicy = projectWorkspacePolicy()
        val catalog =
            PresentationPolicyCatalog { role ->
                if (role == ScreenRoleDto.PROJECT_WORKSPACE) workspacePolicy else null
            }
        assertSame(catalog[ScreenRoleDto.PROJECT_WORKSPACE], catalog[ScreenRoleDto.PROJECT_WORKSPACE])
    }

    @Test
    fun unlistedRole_returnsNull() {
        val queried = mutableListOf<ScreenRoleDto>()
        val catalog =
            PresentationPolicyCatalog { role ->
                queried.add(role)
                null
            }
        assertNull(catalog[ScreenRoleDto.SYNC])
        // 未预解析角色不触发任何解析调用（只查 Map）。
        assertFalse(queried.contains(ScreenRoleDto.SYNC))
    }

    @Test
    fun projectWorkspace_contract_deliversVolumeChapterCreationEntries() {
        val catalog =
            PresentationPolicyCatalog { role ->
                if (role == ScreenRoleDto.PROJECT_WORKSPACE) projectWorkspacePolicy() else null
            }
        val spec = AndroidWorkspaceActionPolicy.resolve(catalog[ScreenRoleDto.PROJECT_WORKSPACE])

        // 卷标题行新建卷（ListHeader/Project）。
        assertTrue(
            spec.listHeaderActions.any {
                it.kind == WorkspaceActionKind.CreateVolume && it.target == WorkspaceActionTarget.Project
            },
        )
        // 卷行尾部新建章节（ItemTrailing/Volume）。
        assertTrue(
            spec.itemTrailingActions(WorkspaceActionTarget.Volume).any {
                it.kind == WorkspaceActionKind.CreateChapter
            },
        )
        // 空卷空态新建章节（EmptyState/Volume）— 截图里"第一卷 / 暂无章节"缺加号的回归点。
        assertTrue(
            spec.emptyStateActions(WorkspaceActionTarget.Volume).any {
                it.kind == WorkspaceActionKind.CreateChapter
            },
        )
    }
}
