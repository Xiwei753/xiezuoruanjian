package com.xiwei.sujian.app.presentation

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
 * #610：AndroidChromePolicy 只消费 Core screen contract（ActionSlot 区域/顺序/业务目标），
 * 不再自建第二份“同步→搜索→设置”或“哪些页面显示顶栏/底栏”的规则。
 *
 * 行为对齐 #597 正文一/三/四 + #610 评论二：
 * - 一级导航只保留 作品/星图/统计；设置从顶栏进入，不是一级入口；
 * - 作品页顶栏右侧产品顺序（从右往左）为 设置/搜索/同步状态，
 *   代码顺序（Core order 升序）为 同步 → 搜索 → 设置；
 * - 进入设置后顶栏只保留左上返回，一级导航（底栏/侧栏）消失；
 * - 进入正文后一级导航消失；写作区顶栏透明、不显示标题、只保留同步/设置
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

    private fun policy(
        screenRole: ScreenRoleDto,
        slots: List<ActionSlotDto>,
    ): ScreenPolicyDto = ScreenPolicyDto(screenRole = screenRole, actionSlots = slots)

    /** 作品页契约：HeaderTrailing = 同步(10)/搜索(20)/设置(30)，无 Sort（#610 评论二）。 */
    private fun projectWorkspacePolicy(): ScreenPolicyDto =
        policy(
            ScreenRoleDto.PROJECT_WORKSPACE,
            listOf(
                slot(ActionRoleDto.SETTINGS, ActionRegionDto.HEADER_TRAILING, 30),
                slot(ActionRoleDto.SEARCH, ActionRegionDto.HEADER_TRAILING, 20),
                slot(ActionRoleDto.SYNC, ActionRegionDto.HEADER_TRAILING, 10),
            ),
        )

    /** 写作页契约：HeaderLeading=Back，HeaderTrailing = 同步(20)/设置(30)，无 Save。 */
    private fun writingPolicy(): ScreenPolicyDto =
        policy(
            ScreenRoleDto.WRITING,
            listOf(
                slot(ActionRoleDto.BACK, ActionRegionDto.HEADER_LEADING, 10),
                slot(ActionRoleDto.SYNC, ActionRegionDto.HEADER_TRAILING, 20),
                slot(ActionRoleDto.SETTINGS, ActionRegionDto.HEADER_TRAILING, 30),
            ),
        )

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
        contractShowsPrimaryNavigation: Boolean = true,
    ): SujianChromeSpec =
        AndroidChromePolicy.resolve(
            screenRole = screenRole,
            screenPolicy = screenPolicy,
            workspaceLocation = location,
            canWorkspaceNavigateBack = canBack,
            contractShowsPrimaryNavigation = contractShowsPrimaryNavigation,
        )

    // ---- 页面角色映射 ----

    @Test
    fun `route to screen role mapping`() {
        assertEquals(
            ScreenRoleDto.PROJECT_WORKSPACE,
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
        // 写作区只保留需要的图标层：同步、设置（Save 已从 Core 契约删除，#610 评论二）。
        assertEquals(
            listOf(SujianChromeAction.Sync, SujianChromeAction.Settings),
            spec.actions,
        )
    }

    // ---- 返回箭头与动作同源（评论问题三） ----

    @Test
    fun `works chapter tree shows back when workspace can navigate back`() {
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
    fun `stats root never shows a back arrow even if workspace could go back`() {
        val spec =
            resolve(
                ScreenRoleDto.STATS,
                policy(ScreenRoleDto.STATS, emptyList()),
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
                canBack = true,
            )
        assertFalse(spec.showBack)
    }

    // ---- #610：一级导航可见性跟随 Core 布局契约 ----

    @Test
    fun `primary navigation hidden when Core contract hides it`() {
        val spec =
            resolve(
                ScreenRoleDto.PROJECT_WORKSPACE,
                projectWorkspacePolicy(),
                contractShowsPrimaryNavigation = false,
            )
        assertFalse("键盘/触控单栏时 Core 隐藏一级导航，Android 不得覆盖", spec.showPrimaryNavigation)
    }

    // ---- 设置页返回来自 Core Back 槽位 ----

    @Test
    fun `settings back requires Core back slot`() {
        val withoutBack =
            resolve(ScreenRoleDto.SETTINGS, policy(ScreenRoleDto.SETTINGS, emptyList()))
        assertFalse("Core 无 Back 槽位时不得显示返回箭头", withoutBack.showBack)
    }

    // ---- #610 评论二：headerActions 只做角色→控件映射，不做动作存在性过滤 ----

    @Test
    fun `non header roles never render as top bar icons`() {
        // 即使契约里混入非 HeaderTrailing 角色（Back/新建/删除/重命名），
        // 它们也只映射到各自区域的控件，绝不进顶栏——这是控件映射不是过滤。
        val policy =
            policy(
                ScreenRoleDto.PROJECT_WORKSPACE,
                listOf(
                    slot(ActionRoleDto.BACK, ActionRegionDto.HEADER_LEADING, 10),
                    slot(ActionRoleDto.DELETE, ActionRegionDto.CONTEXT, 10, requiresConfirmation = true),
                    slot(ActionRoleDto.RENAME, ActionRegionDto.CONTEXT, 20),
                    slot(ActionRoleDto.CREATE_VOLUME, ActionRegionDto.LIST_HEADER, 10),
                    slot(ActionRoleDto.CREATE_CHAPTER, ActionRegionDto.ITEM_TRAILING, 10),
                    slot(ActionRoleDto.SYNC, ActionRegionDto.HEADER_TRAILING, 10),
                ),
            )
        assertEquals(listOf(SujianChromeAction.Sync), AndroidChromePolicy.headerActions(policy))
    }

    @Test
    fun `writing contract without save renders only sync and settings`() {
        // #610 评论二：正文自动保存，Core 契约不再声明 Save；
        // Android 呈现的就是契约里真实存在的动作，没有第二个真相。
        val spec =
            resolve(
                ScreenRoleDto.WRITING,
                writingPolicy(),
                location = WorkspaceLocation.Editor("p1", "v1", "c1"),
            )
        assertEquals(
            listOf(SujianChromeAction.Sync, SujianChromeAction.Settings),
            spec.actions,
        )
    }
}
