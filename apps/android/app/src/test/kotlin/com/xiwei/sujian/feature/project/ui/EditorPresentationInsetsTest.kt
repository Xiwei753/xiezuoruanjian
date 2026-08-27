package com.xiwei.sujian.feature.project.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #640 评论 5441849412 问题3 / 5442422507：Editor host inset 消费策略回归测试。
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
 * #640 评论 5442422507：[EditorHostInsetPolicy] 现在只对 **compact** host 有效
 * （compact 仍用 [editorHostInsetPadding] 套 windowInsetsPadding）。**wide** host 不再套 inset padding
 * （Rust plan 按整窗尺寸算 slot bounds，host 先缩会让 measureAndPlaceWorkbench 把子项撑回原尺寸、
 * 伸进 IME）；wide slot 内容改用 `fitInside(WindowInsetsRulers.SafeDrawing.current)` 在 slot 内部
 * reposition 到 safe region（绝对窗口位置，绕过 ancestor consumption）。本测仍锁住 compact host 策略。
 *
 * 本测锁住不变量：
 * - IME 出现时 compact Editor 可用高度只减少一次（[EditorHostInsetPolicy.consumesIme] = true，且唯一 owner）；
 * - top statusBars 不消费（[EditorHostInsetPolicy.consumesTopStatusBar] = false，
 *   compact top bar / Workbench toolbar 自己处理顶部系统栏，不重复 top inset）；
 * - 底部 navigationBars 和横向 displayCutout 消费（safe drawing 在 compact host 层统一处理）。
 */
class EditorPresentationInsetsTest {
    @Test
    fun imeConsumed_exactlyOnce() {
        val policy = resolveEditorHostInsetPolicy()
        assertTrue(
            "IME 必须由 compact host 消费（Editor 可用高度只减少一次，不是 0 次）",
            policy.consumesIme,
        )
    }

    @Test
    fun topStatusBar_notConsumed_byHost() {
        val policy = resolveEditorHostInsetPolicy()
        assertFalse(
            "top statusBars 不得由 compact host 消费（compact top bar / Workbench toolbar 自己处理顶部，不重复 top inset）",
            policy.consumesTopStatusBar,
        )
    }

    @Test
    fun navigationBars_consumed_byHost() {
        val policy = resolveEditorHostInsetPolicy()
        assertTrue(
            "底部 navigationBars 必须由 compact host 消费（IME 出现时被覆盖，windowInsetsPadding 取 max）",
            policy.consumesNavigationBars,
        )
    }

    @Test
    fun displayCutout_consumed_byHost() {
        val policy = resolveEditorHostInsetPolicy()
        assertTrue(
            "横向 displayCutout 必须由 compact host 消费（折叠屏铰链/刘海）",
            policy.consumesDisplayCutout,
        )
    }

    @Test
    fun imeOwner_isUnique_singleOwnerStrategy() {
        // 锁住"IME 只有一个 owner"策略：compact host 消费 IME，WritingPaneLayout 不再消费。
        // EditorHostInsetPolicy.consumesIme=true 表示 compact host 是唯一 owner。
        // WritingPaneLayout.imePadding 已删除（#640 B.11），不在本测范围，
        // 此处只锁住 compact host 策略：consumesIme=true 且只有一个 EditorHostInsetPolicy 实例决策。
        // #640 评论 5442422507：wide host 不套 inset padding，由 slot fitInside 处理，不在本测范围。
        val policy1 = resolveEditorHostInsetPolicy()
        val policy2 = resolveEditorHostInsetPolicy()
        assertTrue("compact host 必须消费 IME", policy1.consumesIme)
        assertTrue("compact host 必须消费 IME", policy2.consumesIme)
        // 策略是纯函数，两次调用返回相同决策 — 唯一 owner 语义稳定。
        org.junit.Assert.assertEquals(
            "inset 消费策略必须是纯函数稳定决策（唯一 owner）",
            policy1,
            policy2,
        )
    }
}
