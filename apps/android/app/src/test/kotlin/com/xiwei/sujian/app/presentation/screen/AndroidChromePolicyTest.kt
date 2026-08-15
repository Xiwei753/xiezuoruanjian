package com.xiwei.sujian.app.presentation.screen

import com.xiwei.sujian.app.navigation.SujianRoute
import com.xiwei.sujian.feature.project.ui.WorkspaceLocation
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.writer_core.ActionRegionDto
import uniffi.writer_core.ActionRoleDto
import uniffi.writer_core.ActionSlotDto
import uniffi.writer_core.ActionTargetDto
import uniffi.writer_core.ScreenPolicyDto
import uniffi.writer_core.ScreenRoleDto

/**
 * #610 / #628：AndroidChromePolicy 只消费 Core screen contract（ActionSlot 区域/顺序/业务目标
 * + show_primary_navigation），不再自建第二份"同步→搜索→设置"或"哪些页面显示顶栏/底栏"的规则。
 *
 * 行为对齐 #597 正文一/三/四 + #610 评论二 + #628 评论第 5 节：
 * - 一级导航可见性由 Core `ScreenPolicy.show_primary_navigation` 决定（Rust 的
 *   Writing/Settings 返回 false，其余一级页面返回 true）；Android 直接读，
 *   不再传 `contractShowsPrimaryNavigation` 参数；
 * - 作品页顶栏右侧产品顺序（从右往左）为 设置/搜索/同步状态，
 *   代码顺序（Core order 升序）为 同步 → 搜索 → 设置；
 * - 进入设置后顶栏只保留左上返回，一级导航（底栏/侧栏）消失；
 * - 进入正文后一级导航消失；写作区顶栏透明、不显示标题、只保留同步/搜索/设置（#624 评论6 恢复搜索入口）
 *   （Save 已从 Core 契约删除，Android 不再过滤任何动作）；
 * - 星图/统计根页没有返回动作。
 */
class AndroidChromePolicyTest {
    private fun slot(
        role: ActionRoleDto,
        region: ActionRegionDto,
        order: Int,
        requiresConfirmation: Boolean = false,
        target: ActionTargetDto = ActionTargetDto.APP,
    ): ActionSlotDto =
        ActionSlotDto(
            role = role,
            target = target,
            region = region,
            order = order.toUShort(),
            requiresConfirmation = requiresConfirmation,
        )

    /**
     * 构造 ScreenPolicyDto。#628：新增 showPrimaryNavigation 字段，由 Core 决定。
     * 默认 true（与 Core resolve_show_primary_navigation 对齐：除 Writing/Settings 外均为 true）。
     */
    private fun policy(
        screenRole: ScreenRoleDto,
        slots: List<ActionSlotDto>,
        showPrimaryNavigation: Boolean = defaultShowPrimaryNavigation(screenRole),
    ): ScreenPolicyDto =
        ScreenPolicyDto(
            screenRole = screenRole,
            actionSlots = slots,
            showPrimaryNavigation = showPrimaryNavigation,
        )

    /** 与 Core resolve_show_primary_navigation 对齐的默认值（Writing/Settings=false，其余=true）。 */
    private fun defaultShowPrimaryNavigation(screenRole: ScreenRoleDto): Boolean =
        when (screenRole) {
            ScreenRoleDto.WRITING, ScreenRoleDto.SETTINGS -> false
            else -> true
        }

    /**
     * 作品页契约（#610 评论五 / 任务 1 后）：HeaderLeading=Back，HeaderTrailing = 同步(10)/搜索(20)/设置(30)。
     * Back 槽位由 Core 决定，是否真的显示返回还需 Android navigator 历史可回退。
     */
    private fun projectWorkspacePolicy(): ScreenPolicyDto =
        policy(
            ScreenRoleDto.PROJECT_WORKSPACE,
            listOf(
                slot(ActionRoleDto.BACK, ActionRegionDto.HEADER_LEADING, 10),
                slot(ActionRoleDto.SETTINGS, ActionRegionDto.HEADER_TRAILING, 30),
                slot(ActionRoleDto.SEARCH, ActionRegionDto.HEADER_TRAILING, 20),
                slot(ActionRoleDto.SYNC, ActionRegionDto.HEADER_TRAILING, 10),
            ),
        )

    /**
     * 写作页契约（#624 评论6 对齐 Core `screen_contract.rs` ScreenRole::Writing）：
     * HeaderLeading=Back，HeaderTrailing = 同步(10)/搜索(20)/设置(30)，无 Save。
     * 搜索入口由 #477 接管，功能未完成时点击可暂无动作，但图标不得从产品契约消失。
     * #628：Writing 的 showPrimaryNavigation=false（Core 决定）。
     */
    private fun writingPolicy(): ScreenPolicyDto =
        policy(
            ScreenRoleDto.WRITING,
            listOf(
                slot(ActionRoleDto.BACK, ActionRegionDto.HEADER_LEADING, 10),
                slot(ActionRoleDto.SYNC, ActionRegionDto.HEADER_TRAILING, 10),
                slot(ActionRoleDto.SEARCH, ActionRegionDto.HEADER_TRAILING, 20),
                slot(ActionRoleDto.SETTINGS, ActionRegionDto.HEADER_TRAILING, 30),
            ),
        )

    /** #628：Settings 的 showPrimaryNavigation=false（Core 决定）。 */
    private fun settingsPolicy(): ScreenPolicyDto =
        policy(
            ScreenRoleDto.SETTINGS,
            listOf(slot(ActionRoleDto.BACK, ActionRegionDto.HEADER_LEADING, 10)),
        )

    private fun resolve(
        screenRole: ScreenRoleDto,
        screenPolicy: ScreenPolicyDto?,
        location: WorkspaceLocation = WorkspaceLocation.ProjectList,
        canBack: Boolean = false,
    ): SujianChromeSpec =
        AndroidChromePolicy.resolve(
            screenRole = screenRole,
            screenPolicy = screenPolicy,
            workspaceLocation = location,
            canWorkspaceNavigateBack = canBack,
        )

    // ---- 页面角色映射 ----

    @Test
    fun `route to screen role mapping`() {
        // #610 评论四：Works 内部分三个页面角色，Core ProjectList 契约必须有真实消费点。
        assertEquals(
            ScreenRoleDto.PROJECT_LIST,
            AndroidChromePolicy.screenRoleFor(SujianRoute.Works, WorkspaceLocation.ProjectList),
        )
        assertEquals(
            ScreenRoleDto.PROJECT_WORKSPACE,
            AndroidChromePolicy.screenRoleFor(
                SujianRoute.Works,
                WorkspaceLocation.ChapterTree("p1"),
            ),
        )
        assertEquals(
            ScreenRoleDto.WRITING,
            AndroidChromePolicy.screenRoleFor(
                SujianRoute.Works,
                WorkspaceLocation.Editor("p1", "v1", "c1"),
            ),
        )
        assertEquals(
            ScreenRoleDto.SETTINGS,
            AndroidChromePolicy.screenRoleFor(SujianRoute.Settings, WorkspaceLocation.ProjectList),
        )
        assertEquals(
            ScreenRoleDto.STAR_MAP,
            AndroidChromePolicy.screenRoleFor(SujianRoute.StarMap, WorkspaceLocation.ProjectList),
        )
        assertEquals(
            ScreenRoleDto.STATS,
            AndroidChromePolicy.screenRoleFor(SujianRoute.Stats, WorkspaceLocation.ProjectList),
        )
    }

    // ---- 一级导航：作品 / 星图 / 统计 ----

    @Test
    fun `works root keeps primary navigation with sync search settings in Core order`() {
        val spec = resolve(ScreenRoleDto.PROJECT_WORKSPACE, projectWorkspacePolicy())
        assertTrue(spec.showPrimaryNavigation)
        assertFalse(spec.appBarTransparent)
        assertTrue(spec.showTitle)
        assertFalse(spec.showBack)
        // #610：顺序来自 Core order（升序 = 代码顺序 同步 → 搜索 → 设置）；
        // Sort 已从 Core 契约删除（#610 评论二），Android 不需要过滤任何动作。
        assertEquals(
            listOf(SujianChromeAction.Sync, SujianChromeAction.Search, SujianChromeAction.Settings),
            spec.actions,
        )
    }

    @Test
    fun `starmap root keeps primary navigation and has no actions or back`() {
        val spec = resolve(ScreenRoleDto.STAR_MAP, policy(ScreenRoleDto.STAR_MAP, emptyList()))
        assertTrue(spec.showPrimaryNavigation)
        assertTrue(spec.actions.isEmpty())
        assertFalse("星图占位根页没有返回动作（正文四）", spec.showBack)
        assertTrue(spec.showTitle)
    }

    @Test
    fun `stats root keeps primary navigation and never shows back arrow`() {
        val spec = resolve(ScreenRoleDto.STATS, policy(ScreenRoleDto.STATS, emptyList()))
        assertTrue(spec.showPrimaryNavigation)
        assertFalse(spec.showBack)
        assertTrue(spec.actions.isEmpty())
        // 即使 workspace 历史可回退，stats 根页也不显示返回箭头（与 ProjectWorkspace/Editor 不同）。
        assertFalse(
            resolve(
                ScreenRoleDto.STATS,
                policy(ScreenRoleDto.STATS, emptyList()),
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
                canBack = true,
            ).showBack,
        )
    }

    // ---- 设置：顶栏只保留左上返回，无一级导航 ----

    @Test
    fun `settings shows only back and hides primary navigation`() {
        val spec = resolve(ScreenRoleDto.SETTINGS, settingsPolicy())
        assertTrue(spec.showBack)
        assertFalse(spec.showPrimaryNavigation)
        assertTrue(spec.actions.isEmpty())
        assertFalse(spec.appBarTransparent)
        assertTrue(spec.showTitle)
        // Core 无 Back 槽位时不得显示返回箭头（同源：返回来自 Core Back 槽位）。
        val withoutBack =
            resolve(ScreenRoleDto.SETTINGS, policy(ScreenRoleDto.SETTINGS, emptyList()))
        assertFalse("Core 无 Back 槽位时不得显示返回箭头", withoutBack.showBack)
    }

    // ---- 正文：隐藏一级导航、透明顶栏、无标题、只保留需要的图标层 ----

    @Test
    fun `editor hides primary navigation and shows transparent titleless top bar`() {
        val spec =
            resolve(
                ScreenRoleDto.WRITING,
                writingPolicy(),
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
                canBack = true,
            )
        assertFalse("进入正文后隐藏一级导航（底栏/侧栏）", spec.showPrimaryNavigation)
        assertTrue("写作区顶栏透明背景", spec.appBarTransparent)
        assertFalse("写作区顶栏不显示标题", spec.showTitle)
        assertTrue(spec.showBack)
        // 写作区只保留需要的图标层：同步、搜索、设置（Save 已从 Core 契约删除，#610 评论二）。
        assertEquals(
            listOf(SujianChromeAction.Sync, SujianChromeAction.Search, SujianChromeAction.Settings),
            spec.actions,
        )
    }

    // ---- 返回箭头与动作同源（评论问题三） ----

    @Test
    fun `works chapter tree shows back when Core has back and workspace can navigate back`() {
        // #610 评论五：showBack = Core HeaderLeading 有 Back && canWorkspaceNavigateBack。
        val spec =
            resolve(
                ScreenRoleDto.PROJECT_WORKSPACE,
                projectWorkspacePolicy(),
                location = WorkspaceLocation.ChapterTree("p1"),
                canBack = true,
            )
        assertTrue(spec.showBack)
        assertTrue(spec.showPrimaryNavigation)
        assertFalse(spec.appBarTransparent)
    }

    @Test
    fun `show back requires both core back slot and navigation history`() {
        // #610 评论五：showBack = Core HeaderLeading 有 Back && canWorkspaceNavigateBack。
        // 四个反例合并测试，覆盖 ProjectWorkspace / Writing × Core无Back / navigator无历史。
        // ProjectWorkspace：Core 有 Back 但 navigator 无历史 → 不显示。
        assertFalse(
            resolve(
                ScreenRoleDto.PROJECT_WORKSPACE,
                projectWorkspacePolicy(),
                location = WorkspaceLocation.ChapterTree("p1"),
                canBack = false,
            ).showBack,
        )
        // ProjectWorkspace：navigator 可回退但 Core 无 Back → 不显示。
        assertFalse(
            resolve(
                ScreenRoleDto.PROJECT_WORKSPACE,
                policy(
                    ScreenRoleDto.PROJECT_WORKSPACE,
                    listOf(
                        slot(ActionRoleDto.SYNC, ActionRegionDto.HEADER_TRAILING, 10),
                        slot(ActionRoleDto.SETTINGS, ActionRegionDto.HEADER_TRAILING, 30),
                    ),
                ),
                location = WorkspaceLocation.ChapterTree("p1"),
                canBack = true,
            ).showBack,
        )
        // Writing：Core 有 Back 但 navigator 无历史 → 不显示。
        assertFalse(
            resolve(
                ScreenRoleDto.WRITING,
                writingPolicy(),
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
                canBack = false,
            ).showBack,
        )
        // Writing：navigator 可回退但 Core 无 Back → 不显示。
        assertFalse(
            resolve(
                ScreenRoleDto.WRITING,
                policy(
                    ScreenRoleDto.WRITING,
                    listOf(
                        slot(ActionRoleDto.SYNC, ActionRegionDto.HEADER_TRAILING, 20),
                        slot(ActionRoleDto.SETTINGS, ActionRegionDto.HEADER_TRAILING, 30),
                    ),
                ),
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
                canBack = true,
            ).showBack,
        )
    }

    // ---- #628 评论第 5 节：一级导航可见性由 Core ScreenPolicy.show_primary_navigation 决定 ----

    @Test
    fun `primary navigation follows Core screen policy show_primary_navigation`() {
        // #628：一级导航可见性直接读 ScreenPolicy.showPrimaryNavigation（Core 决定）。
        // Android 不再传 contractShowsPrimaryNavigation 参数，也不在此处写
        // when(screenRole) { SETTINGS, WRITING -> false; else -> ... }。
        val visible =
            resolve(
                ScreenRoleDto.PROJECT_WORKSPACE,
                policy(
                    ScreenRoleDto.PROJECT_WORKSPACE,
                    emptyList(),
                    showPrimaryNavigation = true,
                ),
            )
        assertTrue("Core show_primary_navigation=true 时 Android 显示一级导航", visible.showPrimaryNavigation)

        val hidden =
            resolve(
                ScreenRoleDto.PROJECT_WORKSPACE,
                policy(
                    ScreenRoleDto.PROJECT_WORKSPACE,
                    emptyList(),
                    showPrimaryNavigation = false,
                ),
            )
        assertFalse("Core show_primary_navigation=false 时 Android 隐藏一级导航", hidden.showPrimaryNavigation)
    }

    @Test
    fun `null screen policy defaults to showing primary navigation`() {
        // 契约缺失（桥失败/空契约）时 fallback 到 true，避免误隐藏一级导航。
        val spec = resolve(ScreenRoleDto.PROJECT_WORKSPACE, null)
        assertTrue(spec.showPrimaryNavigation)
    }

    // ---- #610 评论二：headerActions 只做角色→控件映射，不做动作存在性过滤 ----

    @Test
    fun `non header roles never render as top bar icons`() {
        // 即使契约里混入非 HeaderTrailing 角色（Back/新建/删除/重命名/上移/下移），
        // 它们也只映射到各自区域的控件，绝不进顶栏——这是控件映射不是过滤。
        val policy =
            policy(
                ScreenRoleDto.PROJECT_WORKSPACE,
                listOf(
                    slot(ActionRoleDto.BACK, ActionRegionDto.HEADER_LEADING, 10),
                    slot(ActionRoleDto.DELETE, ActionRegionDto.CONTEXT, 10, requiresConfirmation = true),
                    slot(ActionRoleDto.RENAME, ActionRegionDto.CONTEXT, 20),
                    slot(ActionRoleDto.MOVE_EARLIER, ActionRegionDto.CONTEXT, 30),
                    slot(ActionRoleDto.MOVE_LATER, ActionRegionDto.CONTEXT, 40),
                    slot(ActionRoleDto.CREATE_VOLUME, ActionRegionDto.LIST_HEADER, 10),
                    slot(ActionRoleDto.CREATE_CHAPTER, ActionRegionDto.ITEM_TRAILING, 10),
                    slot(ActionRoleDto.SYNC, ActionRegionDto.HEADER_TRAILING, 10),
                ),
            )
        assertEquals(listOf(SujianChromeAction.Sync), AndroidChromePolicy.headerActions(policy))
    }

    @Test
    fun `writing contract without save renders sync search and settings`() {
        // #610 评论二：正文自动保存，Core 契约不再声明 Save；#624 评论6：写作页顶栏恢复搜索入口。
        // Android 呈现的就是契约里真实存在的动作，没有第二个真相。
        val spec =
            resolve(
                ScreenRoleDto.WRITING,
                writingPolicy(),
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
            )
        assertEquals(
            listOf(SujianChromeAction.Sync, SujianChromeAction.Search, SujianChromeAction.Settings),
            spec.actions,
        )
    }
}
