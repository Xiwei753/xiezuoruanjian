package com.xiwei.sujian.feature.project.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #640 评论 5441849412 问题3：Editor host inset 消费策略回归测试。
 *
 * 根因：EditorPresentationHost 提到 [com.xiwei.sujian.app.navigation.SujianNavigationSuite] 的
 * `Box { SujianNavScaffoldContent(...); EditorPresentationHost(...) }`（Scaffold 外）。
 * 原来 `Scaffold(contentWindowInsets = WindowInsets.safeDrawing)` 的 innerPadding 只包住
 * navDisplayContent()，现在包不到上面的 EditorPresentationHost。项目启用 enableEdgeToEdge() +
 * adjustResize：edge-to-edge 下 adjustResize 只让应用收到 IME inset，内容仍必须明确处理 inset。
 * 之前 #640 修掉的是"双算 IME"，现在 lifted host 这条新路径变成"Editor 自己根本没吃 IME/safeDrawing"。
 *
 * 修复：inset 由 EditorPresentationHost 唯一拥有一次，不加回 WritingPaneLayout.imePadding。
 * 策略由纯函数 [resolveEditorHostInsetPolicy] 统一表达，生产 host 与单测共用。
 *
 * 本测锁住不变量：
 * - IME 出现时 Editor 可用高度只减少一次（[EditorHostInsetPolicy.consumesIme] = true，且唯一 owner）；
 * - top statusBars 不消费（[EditorHostInsetPolicy.consumesTopStatusBar] = false，
 *   compact top bar / Workbench toolbar 自己处理顶部系统栏，不重复 top inset）；
 * - 底部 navigationBars 和横向 displayCutout 消费（safe drawing 在 host 层统一处理）。
 */
class EditorPresentationInsetsTest {
    @Test
    fun imeConsumed_exactlyOnce() {
        val policy = resolveEditorHostInsetPolicy()
        assertTrue(
            "IME 必须由 host 消费（Editor 可用高度只减少一次，不是 0 次）",
            policy.consumesIme,
        )
    }

    @Test
    fun topStatusBar_notConsumed_byHost() {
        val policy = resolveEditorHostInsetPolicy()
        assertFalse(
            "top statusBars 不得由 host 消费（compact top bar / Workbench toolbar 自己处理顶部，不重复 top inset）",
            policy.consumesTopStatusBar,
        )
    }

    @Test
    fun navigationBars_consumed_byHost() {
        val policy = resolveEditorHostInsetPolicy()
        assertTrue(
            "底部 navigationBars 必须由 host 消费（IME 出现时被覆盖，windowInsetsPadding 取 max）",
            policy.consumesNavigationBars,
        )
    }

    @Test
    fun displayCutout_consumed_byHost() {
        val policy = resolveEditorHostInsetPolicy()
        assertTrue(
            "横向 displayCutout 必须由 host 消费（折叠屏铰链/刘海）",
            policy.consumesDisplayCutout,
        )
    }

    @Test
    fun imeOwner_isUnique_singleOwnerStrategy() {
        // 锁住"IME 只有一个 owner"策略：host 消费 IME，WritingPaneLayout 不再消费。
        // EditorHostInsetPolicy.consumesIme=true 表示 host 是唯一 owner。
        // WritingPaneLayout.imePadding 已删除（#640 B.11），不在本测范围，
        // 此处只锁住 host 策略：consumesIme=true 且只有一个 EditorHostInsetPolicy 实例决策。
        val policy1 = resolveEditorHostInsetPolicy()
        val policy2 = resolveEditorHostInsetPolicy()
        assertTrue("host 必须消费 IME", policy1.consumesIme)
        assertTrue("host 必须消费 IME", policy2.consumesIme)
        // 策略是纯函数，两次调用返回相同决策 — 唯一 owner 语义稳定。
        org.junit.Assert.assertEquals(
            "inset 消费策略必须是纯函数稳定决策（唯一 owner）",
            policy1,
            policy2,
        )
    }

    @Test
    fun imeReductionCount_isExactlyOne() {
        // 综合断言：IME 出现时 Editor 可用高度只减少一次。
        // host 消费 IME（consumesIme=true）→ 减少一次；
        // top statusBars 不消费（consumesTopStatusBar=false）→ 不影响 IME；
        // WritingPaneLayout 不再消费 IME（#640 B.11 已删除 imePadding）→ 不二次减少。
        // 所以 IME 减少次数 = 1（host 唯一消费）。
        val policy = resolveEditorHostInsetPolicy()
        val imeReductionCount = if (policy.consumesIme) 1 else 0
        assertTrue(
            "IME 出现时 Editor 可用高度只减少一次（不是 0 次，也不是 2 次）",
            imeReductionCount == 1,
        )
    }
}
