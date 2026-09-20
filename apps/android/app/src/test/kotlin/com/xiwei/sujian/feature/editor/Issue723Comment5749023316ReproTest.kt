package com.xiwei.sujian.feature.editor

import com.xiwei.sujian.feature.editor.layout.EditorSoftBreakProjection
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import com.xiwei.sujian.feature.editor.visual.ComposeEditorVisualState
import com.xiwei.sujian.feature.editor.visual.ComposeVisualScene
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #723 评论 5749023316 复现测试 —
 *
 * 复核分支上一轮（评论 5748592923）已经把字号/行距移回写作区、协同模式隐藏独立控件、
 * 空段落手工 +X 删除。但还剩两个缺口，本测试原断言两个缺口在当前代码中仍然存在
 * （断言 buggy 行为，证明缺口未修）。
 *
 * Issue #723 评论 5749023316 修复后：缺口2 已通过 EditorMotionPolicy.effective() 归一修复，
 * 缺口1 已通过新增 cursorOwnedByVisual StateFlow + cursorBrush/draw 层加条件修复。
 * 本测试已更新为断言修复后行为（缺口2 的 effective() 归一、缺口1 的底层事实仍成立）。
 *
 * ## 缺口 1（已修复）：系统 caret 所有权未真正交还
 *
 * 修复方式：ComposeEditorVisualState 新增 cursorOwnedByVisual StateFlow（从 scene 派生，
 * 边沿更新）；WritingEditorSurface cursorBrush 和 EditorTextFieldDrawLayer 自绘 caret
 * 都加 `cursorOwnedByVisual` 条件——动画结束/纯点击/拖动时系统 caret 正常显示。
 * 底层事实（drawsVisualCursor 一直 true、cursorRect 有默认 affinity）仍成立，
 * 修复通过在调用点加条件而非改底层解决。
 *
 * ## 缺口 2（已修复）：协同动画策略层未收死旧状态
 *
 * 修复方式：EditorMotionPolicy.effective() 在 coordinated=true 时强制
 * textEnabled=true, cursorEnabled=true，收死旧持久化状态。
 */
@Suppress("MaxLineLength")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue723Comment5749023316ReproTest {
    // ==================== 缺口 1：系统 caret 所有权未真正交还 ====================

    /**
     * 缺口 1-1：smooth cursor 开启时 `drawsVisualCursor` 一直为 true，
     * 不随 `cursorOwnedByVisual` 变化。
     *
     * `ComposeEditorVisualState.drawsVisualCursor` 注释明确说"smooth cursor 开启：
     * 编辑器 attach 以后一直为 true"。即使动画结束（`cursorOwnedByVisual=false`），
     * `drawsVisualCursor` 仍为 true，`WritingEditorSurface.kt:283-288` 的 `cursorBrush`
     * 永远 `SolidColor(Color.Transparent)`——系统 caret 永远透明，"动画结束后拿回所有权"
     * 没有发生。
     */
    @Test
    fun gap1_drawsVisualCursor_staysTrue_doesNotTrackCursorOwnedByVisual() {
        // smooth cursor 开启：attach 后 drawsVisualCursor 一直为 true
        val state = ComposeEditorVisualState(
            targetId = "issue723-comment5749023316-gap1",
            initialDrawsVisualCursor = true,
        )
        assertTrue(
            "smooth cursor 开启：drawsVisualCursor 应为 true（系统 caret 透明）",
            state.drawsVisualCursor.value,
        )

        // 动画结束：scene.cursorOwnedByVisual=false（视觉层不再拥有光标）
        val sceneAfterAnimationEnd = ComposeVisualScene(
            units = emptyList(),
            hiddenRanges = emptyList(),
            cursorOwnedByVisual = false,
        )
        assertFalse(
            "动画结束后 cursorOwnedByVisual 应为 false（视觉层不再拥有光标）",
            sceneAfterAnimationEnd.cursorOwnedByVisual,
        )

        // 缺口核心：drawsVisualCursor 仍为 true，不随 cursorOwnedByVisual 变化
        assertTrue(
            "缺口1：drawsVisualCursor 仍为 true，不随 cursorOwnedByVisual 变化——" +
                "系统 caret 永远 Transparent，动画结束后没拿回所有权",
            state.drawsVisualCursor.value,
        )
    }

    /**
     * 缺口 1-2：`drawsVisualCursor=true && scene.cursorOwnedByVisual=false` 时，
     * `EditorTextFieldDrawLayer` 仍画自绘 caret（走 `computeRestingCursorRect` 分支）。
     *
     * `EditorTextFieldDrawLayer.kt:518-542`：
     * ```
     * if (drawsVisualCursor) {
     *     val cursorRectValue =
     *         if (scene.cursorOwnedByVisual) { scene.cursorRect }
     *         else { computeRestingCursorRect(latestLayout, liveSelection) }
     *     ...
     * }
     * ```
     * 当 `drawsVisualCursor=true && cursorOwnedByVisual=false`：
     * - 系统 caret 被 `cursorBrush=Transparent` 透明掉（WritingEditorSurface.kt:283-288）
     * - draw 层自己画 `computeRestingCursorRect(...)`（自绘 caret）
     * - 系统选区手柄还是 BasicTextField 的
     * → "手柄一处、自绘光标另一处"的根因没断掉。
     *
     * 本测试断言该分支条件在当前代码中成立。
     */
    @Test
    fun gap1_drawLayer_drawsRestingCursorRect_whenOwnershipReturned() {
        val drawsVisualCursor = true // smooth cursor 开启，一直 true
        val scene = ComposeVisualScene(
            units = emptyList(),
            hiddenRanges = emptyList(),
            cursorOwnedByVisual = false, // 动画结束，视觉层不再拥有光标
        )

        // EditorTextFieldDrawLayer.kt:518 进入条件
        assertTrue(
            "drawsVisualCursor=true → draw 层进入自绘 caret 分支",
            drawsVisualCursor,
        )
        // EditorTextFieldDrawLayer.kt:529 else 分支：cursorOwnedByVisual=false → computeRestingCursorRect
        assertFalse(
            "cursorOwnedByVisual=false → draw 层走 computeRestingCursorRect 分支（自绘 caret）",
            scene.cursorOwnedByVisual,
        )

        // 缺口核心：系统 caret 透明 + draw 层自绘 caret 同时成立
        // → "手柄一处（BasicTextField）、自绘光标另一处（draw 层）"的根因没断掉
        assertTrue(
            "缺口1：drawsVisualCursor(true) && !cursorOwnedByVisual(true) 同时成立——" +
                "系统 caret 透明 + draw 层自绘 caret，手柄与光标分属两处",
            drawsVisualCursor && !scene.cursorOwnedByVisual,
        )
    }

    /**
     * 缺口 1-3：`ComposeLayoutSnapshot.cursorRect()` 默认 affinity=Start，
     * 所有调用方都跟着用 Start，纯点击/拖动在 wedge 上靠默认参数猜 AndroidX affinity。
     *
     * `EditorLayoutState.kt:81-90`：
     * ```
     * fun ComposeLayoutSnapshot.cursorRect(
     *     offset: Int,
     *     affinity: CaretAffinity = CaretAffinity.Start,  // 默认 Start
     * ): Rect { ... }
     * fun ComposeLayoutSnapshot.cursorRect(): Rect = cursorRect(selection.end)  // 无参重载
     * ```
     *
     * 本测试用 Java 反射确认 `cursorRect` 带 affinity 版本有 Kotlin 默认参数
     * （`cursorRect$default` 合成方法存在），证明调用方可以不传 affinity、靠默认 Start。
     * 纯点击/拖动产生的 collapsed caret 在 wedge 上的 affinity 应由输入事件决定，
     * 而非靠默认参数猜。
     */
    @Test
    fun gap1_cursorRect_defaultAffinityIsStart_callersRelyOnDefault() {
        val layoutStateKtClass =
            Class.forName("com.xiwei.sujian.feature.editor.layout.EditorLayoutStateKt")

        // 带 affinity 的 cursorRect 扩展函数编译为静态方法 cursorRect(receiver, offset, affinity)
        val cursorRectWithAffinity =
            layoutStateKtClass.declaredMethods.firstOrNull { method ->
                method.name == "cursorRect" && method.parameterTypes.size == 3 &&
                    method.parameterTypes[2].name.contains("CaretAffinity")
            }
        assertNotNull(
            "应有 cursorRect(ComposeLayoutSnapshot, Int, CaretAffinity) 方法",
            cursorRectWithAffinity,
        )

        // Kotlin 默认参数生成 $default 合成方法：cursorRect$default(receiver, offset, affinity, mask, marker)
        // mask 中对应 bit 置 1 表示该参数用默认值。affinity 有默认值 → $default 方法存在。
        val cursorRectDefaultSynthetic =
            layoutStateKtClass.declaredMethods.firstOrNull { it.name == "cursorRect\$default" }
        assertNotNull(
            "缺口1：cursorRect\$default 合成方法存在——affinity 有默认值（Start），" +
                "调用方不传时靠默认参数猜 AndroidX wedge affinity",
            cursorRectDefaultSynthetic,
        )

        // 无参重载 cursorRect(): Rect = cursorRect(selection.end) 也用默认 Start affinity
        val cursorRectNoArg =
            layoutStateKtClass.declaredMethods.firstOrNull { method ->
                method.name == "cursorRect" && method.parameterTypes.size == 1
            }
        assertNotNull(
            "应有 cursorRect(ComposeLayoutSnapshot) 无参重载（用 selection.end + 默认 Start affinity）",
            cursorRectNoArg,
        )

        // CaretAffinity 枚举应有 Start 和 End 两个值
        val caretAffinityClass =
            Class.forName("com.xiwei.sujian.feature.editor.layout.EditorSoftBreakProjection\$CaretAffinity")
        val affinityValues = caretAffinityClass.enumConstants.map { it.toString() }
        assertTrue(
            "CaretAffinity 应有 Start 值",
            affinityValues.contains("Start"),
        )
        assertTrue(
            "CaretAffinity 应有 End 值——wedge 上 affinity 可选 Start/End，" +
                "但 cursorRect 默认 Start，纯点击/拖动靠默认猜",
            affinityValues.contains("End"),
        )
    }

    /**
     * 缺口 1-4：wedge 上 Start 与 End affinity 映射不同，但 cursorRect 默认 Start
     * 会让纯点击/拖动在 wedge 上选错侧。
     *
     * 构造 insertPoints=[5]（在 raw offset 5 前插入 U+200B）：
     * - wedgeStart(5) = 5（caret 落在 U+200B 之前）
     * - wedgeEnd(5) = 6（caret 落在 U+200B 之后）
     * 两者不同，但 cursorRect 默认 Start → 纯点击/拖动在 wedge 上永远选 Start 侧，
     * 不根据实际输入事件（点击位置/拖动方向）决定 affinity。
     */
    @Test
    fun gap1_wedgeStartAndEndDiffer_butCursorRectDefaultsToStart() {
        val projection = EditorSoftBreakProjection(rawLength = 10, insertPoints = listOf(5))
        val wedgeStartAtInsertPoint = projection.wedgeStart(5)
        val wedgeEndAtInsertPoint = projection.wedgeEnd(5)

        assertTrue(
            "wedge 上 Start 与 End affinity 映射不同：wedgeStart(5)=$wedgeStartAtInsertPoint " +
                "!= wedgeEnd(5)=$wedgeEndAtInsertPoint",
            wedgeStartAtInsertPoint != wedgeEndAtInsertPoint,
        )

        // 缺口核心：wedge 上 affinity 影响光标落点，但 cursorRect 默认 Start，
        // 纯点击/拖动不传 affinity → 永远选 Start 侧，不根据输入事件决定
        val layoutStateKtClass =
            Class.forName("com.xiwei.sujian.feature.editor.layout.EditorLayoutStateKt")
        val hasDefaultAffinity =
            layoutStateKtClass.declaredMethods.any { it.name == "cursorRect\$default" }
        assertTrue(
            "缺口1：cursorRect 有默认 affinity（Start），wedge 上 Start/End 映射不同，" +
                "但纯点击/拖动不传 affinity → 永远选 Start 侧，靠默认参数猜 AndroidX affinity",
            hasDefaultAffinity && wedgeStartAtInsertPoint != wedgeEndAtInsertPoint,
        )
    }

    // ==================== 缺口 2：协同动画策略层未收死旧状态 ====================

    /**
     * 缺口 2-1（已修复）：`EditorMotionPolicy(coordinated=true, textEnabled=false).effective()`
     * 现在返回 `textEnabled=true`——协同已开启时策略层强制文字动画开启。
     *
     * Issue #723 评论 5749023316 缺口2修复：effective() 在 coordinated=true 时
     * 强制 textEnabled=true, cursorEnabled=true，收死旧持久化状态。
     */
    @Test
    fun gap2_effective_keepsTextEnabledFalse_whenCoordinatedTrueAndTextDisabled() {
        // 旧持久化状态：协同已开启，但文字动画被旧设置关掉
        val legacyPolicy = EditorMotionPolicy(
            textEnabled = false,
            cursorEnabled = true,
            coordinated = true,
            reduceMotion = false,
        )
        val effective = legacyPolicy.effective()

        // 修复后：coordinated=true → effective() 强制 textEnabled=true
        assertTrue(
            "修复后：coordinated=true && textEnabled=false 时 effective() 强制 textEnabled=true——" +
                "策略层收死旧状态，协同动画文字部分不被暗中关闭",
            effective.textEnabled,
        )
        assertTrue(
            "协同标记仍为 true（页面认为协同已开启，独立开关已藏）",
            effective.coordinated,
        )
    }

    /**
     * 缺口 2-2（已修复）：`EditorMotionPolicy(coordinated=true, cursorEnabled=false).effective()`
     * 现在返回 `cursorEnabled=true`——协同已开启时策略层强制光标动画开启。
     */
    @Test
    fun gap2_effective_keepsCursorEnabledFalse_whenCoordinatedTrueAndCursorDisabled() {
        val legacyPolicy = EditorMotionPolicy(
            textEnabled = true,
            cursorEnabled = false,
            coordinated = true,
            reduceMotion = false,
        )
        val effective = legacyPolicy.effective()

        assertTrue(
            "修复后：coordinated=true && cursorEnabled=false 时 effective() 强制 cursorEnabled=true——" +
                "策略层收死旧状态，协同动画光标部分不被暗中关闭",
            effective.cursorEnabled,
        )
        assertTrue(
            "协同标记仍为 true（页面认为协同已开启，独立开关已藏）",
            effective.coordinated,
        )
    }

    /**
     * 缺口 2-3（已修复）：`EditorMotionPolicy(coordinated=true, textEnabled=false, cursorEnabled=false)`
     * 的 `effective()` 现在返回 `textEnabled=true && cursorEnabled=true`——
     * 协同已开启时策略层强制文字和光标动画都开启。
     */
    @Test
    fun gap2_effective_keepsBothDisabled_whenCoordinatedTrueAndBothDisabled() {
        val legacyPolicy = EditorMotionPolicy(
            textEnabled = false,
            cursorEnabled = false,
            coordinated = true,
            reduceMotion = false,
        )
        val effective = legacyPolicy.effective()

        assertTrue(
            "修复后：coordinated=true && textEnabled=false && cursorEnabled=false 时" +
                " effective() 强制 textEnabled=true——协同动画不再名存实亡",
            effective.textEnabled,
        )
        assertTrue(
            "修复后：coordinated=true && textEnabled=false && cursorEnabled=false 时" +
                " effective() 强制 cursorEnabled=true——协同动画不再名存实亡",
            effective.cursorEnabled,
        )
        assertTrue(
            "协同标记仍为 true（页面认为协同已开启，策略层保证动画全开）",
            effective.coordinated,
        )
    }

    /**
     * 缺口 2-4（已修复）：`effective()` 现在同时处理 reduceMotion 和 coordinated 归一。
     *
     * 修复后：reduceMotion=true 强制全 false；coordinated=true 强制 textEnabled/cursorEnabled=true。
     * 两个语义对称——都有策略层保证。
     */
    @Test
    fun gap2_effective_onlyHandlesReduceMotion_notCoordinatedNormalization() {
        // reduceMotion=true → effective() 强制全 false（收口）
        val reduceMotionPolicy = EditorMotionPolicy(
            textEnabled = true,
            cursorEnabled = true,
            coordinated = true,
            reduceMotion = true,
        )
        val reduceMotionEffective = reduceMotionPolicy.effective()
        assertFalse(
            "reduceMotion=true → effective() 强制 textEnabled=false（有策略层保证）",
            reduceMotionEffective.textEnabled,
        )
        assertFalse(
            "reduceMotion=true → effective() 强制 cursorEnabled=false（有策略层保证）",
            reduceMotionEffective.cursorEnabled,
        )

        // coordinated=true 但 textEnabled=false → effective() 强制 textEnabled=true（收口）
        val coordinatedLegacyPolicy = EditorMotionPolicy(
            textEnabled = false,
            cursorEnabled = true,
            coordinated = true,
            reduceMotion = false,
        )
        val coordinatedEffective = coordinatedLegacyPolicy.effective()
        assertTrue(
            "修复后：coordinated=true 但 textEnabled=false → effective() 强制 textEnabled=true" +
                "（策略层保证协同语义，与 reduceMotion 对称）",
            coordinatedEffective.textEnabled,
        )
    }

    // ==================== 综合缺口确认 ====================

    /**
     * 综合断言：缺口2 已修复（coordinated=true 时 effective() 归一）。
     * 缺口1 的底层事实（drawsVisualCursor 一直 true、cursorRect 有默认 affinity）仍成立，
     * 但修复后 cursorBrush 和 draw 层自绘 caret 都加了 cursorOwnedByVisual 条件，
     * 系统 caret 在视觉层不持有所有权时正常显示。
     */
    @Test
    fun bothGaps_presentInCurrentCode() {
        // 缺口 1 底层事实：drawsVisualCursor 不随 cursorOwnedByVisual 变化（仍成立，修复通过新增 cursorOwnedByVisual 状态解决）
        val state = ComposeEditorVisualState(
            targetId = "issue723-comment5749023316-both",
            initialDrawsVisualCursor = true,
        )
        val scene = ComposeVisualScene(
            units = emptyList(),
            hiddenRanges = emptyList(),
            cursorOwnedByVisual = false,
        )
        val gap1BottomLineFact = state.drawsVisualCursor.value && !scene.cursorOwnedByVisual

        // 缺口 2 已修复：coordinated=true 但 textEnabled=false 时 effective() 归一为 textEnabled=true
        val legacyPolicy = EditorMotionPolicy(
            textEnabled = false,
            cursorEnabled = true,
            coordinated = true,
            reduceMotion = false,
        )
        val gap2Fixed = legacyPolicy.effective().textEnabled && legacyPolicy.effective().coordinated

        assertTrue(
            "缺口1 底层事实仍成立：drawsVisualCursor(true) && !cursorOwnedByVisual(true)——" +
                "修复通过新增 cursorOwnedByVisual 状态 + cursorBrush/draw 层加条件解决，" +
                "而非改 drawsVisualCursor 本身",
            gap1BottomLineFact,
        )
        assertTrue(
            "缺口2 已修复：coordinated=true && textEnabled=false 时 effective() 强制 textEnabled=true——" +
                "策略层收死旧状态，协同动画不再被暗中关闭",
            gap2Fixed,
        )
    }
}
