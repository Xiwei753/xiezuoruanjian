package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.input.EditorInputSnapshot
import com.xiwei.sujian.feature.editor.input.InputSnapshotOutcome
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.layout.cursorRect
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File

/**
 * Issue #728 评论 5755336403 五个缺口的回归测试。
 *
 * 缺口1：纯 selection 判断用 lastResolvedText（不是 layout text）—
 *   [ComposeEditorVisualState.onInputSnapshotResolved] 的 isPureSelectionMove 判断
 *   `snapshot.text == lastResolvedText`，lastResolvedText 初始为 null。
 *   旧实现用 `layout.text == snapshot.text`，正常打字（layout 先到，snapshot 后到）时
 *   layout.text == snapshot.text 成立，误判为纯 selection，清掉 activeEditMotion。
 *   修复后 lastResolvedText 初始 null，snapshot.text("a") != null，不进入纯 selection 分支。
 *
 * 缺口2：纯 selection 走 pending caret target + 真实 frameTime —
 *   onInputSnapshotResolved 不直接创建 forSelectionMove motion（没有 frameTimeNanos），
 *   只记 pendingSelectionCaretTarget，hasPendingPatches() 为 true 触发帧循环，
 *   drainPendingPatchesAtFrame 用真实 frameTime 创建/重定向 forSelectionMove。
 *
 * 缺口3：handoff 不提前画 target caret —
 *   publishLocalHandoffScene 用 patch.originCaretRect（编辑前位置）填 drawSnapshotState.caretRect，
 *   不用 targetCaretRect。到真实 frameTime 时 drainPendingPatchesAtFrame 创建 motion 推进。
 *
 * 缺口4：Enter 按 text edit 处理 —
 *   drainPendingPatchesAtFrame 的 isSelectionOnly 判断用 `oldText == newText`，
 *   Enter（"a" -> "a\n"）oldText != newText，走 forEdit 分支用 textDurationMillis，
 *   不用 cursorDurationMillis。
 *
 * 缺口5a：smooth cursor 读真实独立设置字段 —
 *   EditorSettingsOps.loadEditorSettingsSnapshot 的 smoothCursorEnabled 读
 *   settings.editorSmoothCursorEnabled，smoothCursorDurationMs 读 settings.editorSmoothCursorDurationMs，
 *   不跟 typing animation 共用 editorTypingAnimationEnabled / editorTypingAnimationDurationMs。
 *
 * 缺口5b：policy 切换保留 resting caret —
 *   updateMotionPolicy settle 动画但不清 restingCaretRect / drawSnapshotState.caretRect，
 *   系统 caret 已透明时自绘 caret 不消失。
 *
 * 测试基础设施：Robolectric + createComposeRule 构造真实 TextLayoutResult（cursorRect 需要）。
 * 用反射访问 ComposeEditorVisualState / ComposeEditMotion 的私有字段。
 */
@Suppress("MaxLineLength", "StringLiteralDuplication", "LongMethod")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue728Comment5755336403ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 缺口1：纯 selection 判断用 lastResolvedText ====================

    /**
     * 缺口1：onInputSnapshotResolved 的纯 selection 判断用 lastResolvedText（不是 layout text）。
     *
     * 场景：正常打字 "" -> "a"，先 onAuthoritativeLayout 让 layout 先到，
     * 再 onInputSnapshotResolved(snapshot text="a", selection=(1,1), outcome=NoTextChange)。
     *
     * 旧实现：layout.text("a") == snapshot.text("a") 成立，误判为纯 selection，清掉 activeEditMotion。
     * 修复后：lastResolvedText 初始为 null，snapshot.text("a") != lastResolvedText(null)，
     *   不进入纯 selection 分支，不清 activeEditMotion。
     *
     * 验证：先建立 activeEditMotion（drainPendingPatchesAtFrame），再调 onInputSnapshotResolved，
     *   activeEditMotion 应保持非 null。同时验证 lastResolvedText 字段存在且被更新。
     */
    @Test
    fun gap1_pureSelectionCheck_usesLastResolvedText_notLayoutText() {
        // 反射验证 lastResolvedText 字段存在且类型为 String（nullable 在 JVM 映射为 String）
        val lastResolvedTextField =
            ComposeEditorVisualState::class.java.declaredFields.firstOrNull { it.name == "lastResolvedText" }
        assertNotNull("ComposeEditorVisualState 应有 lastResolvedText 字段（缺口1 修复）", lastResolvedTextField)
        assertEquals(
            "lastResolvedText 字段类型应是 String",
            String::class.java,
            lastResolvedTextField!!.type,
        )

        val layouts = captureLayouts("", "a")
        val state =
            ComposeEditorVisualState(
                targetId = "issue728-5755336403-gap1",
                classifier = FakeLocalVisualPlanClassifier,
            )
        // 建立基线：空文本，caret 在 offset 0
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        // 插入 "a"
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )
        // onAuthoritativeLayout 配对生成 local patch 并发布 handoff scene，patch 入队
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0, compositionActive = false)
        // drain 创建 activeEditMotion（forEdit，因为 oldText("") != newText("a")）
        state.drainPendingPatchesAtFrame(0L)
        val motionBefore = stateField(state, "activeEditMotion")
        assertNotNull("drain 后 activeEditMotion 应非 null（forEdit 已创建）", motionBefore)

        // 调 onInputSnapshotResolved：text="a"，selection=(1,1)，outcome=NoTextChange。
        // 此时 layout.text("a") == snapshot.text("a")，但 lastResolvedText 初始为 null。
        // 旧实现：layout.text == snapshot.text → 误判纯 selection → 清 activeEditMotion。
        // 修复后：snapshot.text("a") != lastResolvedText(null) → 不进入纯 selection 分支。
        state.onInputSnapshotResolved(
            snapshot = EditorInputSnapshot(text = "a", selection = TextRange(1, 1), composition = null),
            outcome = InputSnapshotOutcome.NoTextChange,
        )
        val motionAfter = stateField(state, "activeEditMotion")
        assertNotNull(
            "onInputSnapshotResolved 后 activeEditMotion 不应被清空（lastResolvedText null 不误判纯 selection）",
            motionAfter,
        )

        // 验证 lastResolvedText 在 onInputSnapshotResolved 后被更新为 snapshot.text
        val lastResolvedTextAfter = stateField(state, "lastResolvedText")
        assertEquals(
            "lastResolvedText 应在 onInputSnapshotResolved 后更新为 snapshot.text",
            "a",
            lastResolvedTextAfter,
        )
    }

    // ==================== 缺口2：纯 selection 走 pending caret target + 真实 frameTime ====================

    /**
     * 缺口2：纯 selection 移动记录 pendingSelectionCaretTarget，在真实 frameTime 创建 forSelectionMove motion。
     *
     * 场景：
     * 1. onAuthoritativeLayout("ab", selection=(0,0)) 设置基线
     * 2. onInputSnapshotResolved(text="ab", selection=(0,0), NoTextChange) 建立 lastResolvedText/lastResolvedSelection
     * 3. onInputSnapshotResolved(text="ab", selection=(1,1), NoTextChange) — 纯 selection 移动
     * 4. hasPendingPatches() == true（pendingSelectionCaretTarget 非 null）
     * 5. drainPendingPatchesAtFrame(0L) 创建 forSelectionMove motion
     * 6. sampleVisualScene(0L) — progress=0，caret 在 origin
     * 7. sampleVisualScene(durationNanos) — progress=1，caret 在 target
     *
     * 关键验证点：
     * - onInputSnapshotResolved 后 hasPendingPatches() 为 true（即使 pendingPatches 为空）
     * - drainPendingPatchesAtFrame 后 activeEditMotion 非 null
     * - motion 用真实 frameTime 创建（sample(0L) 时 caret 在 origin 而非 target）
     */
    @Test
    fun gap2_pureSelectionMove_recordsPendingCaretTarget_andCreatesMotionAtFrameTime() {
        val layouts = captureLayouts("ab")
        val state =
            ComposeEditorVisualState(
                targetId = "issue728-5755336403-gap2",
                classifier = FakeLocalVisualPlanClassifier,
            )
        val cursorDurationMs = 100L
        val cursorDurationNanos = cursorDurationMs * NANOS_PER_MS
        // 设置 cursor 动画开启的 policy（coordinated=true → effective 后 cursorEnabled=true）
        state.updateMotionPolicy(
            EditorMotionPolicy(
                textEnabled = true,
                textDurationMillis = 200L,
                cursorEnabled = true,
                cursorDurationMillis = cursorDurationMs,
                coordinated = true,
            ),
        )
        // 基线 layout："ab"，selection=(0,0)
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        // 第一次 onInputSnapshotResolved：建立 lastResolvedText="ab", lastResolvedSelection=(0,0)
        state.onInputSnapshotResolved(
            snapshot = EditorInputSnapshot(text = "ab", selection = TextRange(0, 0), composition = null),
            outcome = InputSnapshotOutcome.NoTextChange,
        )
        // 第二次 onInputSnapshotResolved：纯 selection 移动到 (1,1)
        state.onInputSnapshotResolved(
            snapshot = EditorInputSnapshot(text = "ab", selection = TextRange(1, 1), composition = null),
            outcome = InputSnapshotOutcome.NoTextChange,
        )
        // 验证 hasPendingPatches() 为 true（pendingSelectionCaretTarget 非 null，即使 pendingPatches 为空）
        assertTrue(
            "纯 selection 移动后 hasPendingPatches() 应为 true（pendingSelectionCaretTarget 触发帧循环）",
            state.hasPendingPatches(),
        )

        // drain 创建 forSelectionMove motion（用真实 frameTime=0L）
        state.drainPendingPatchesAtFrame(0L)
        val motion = stateField(state, "activeEditMotion")
        assertNotNull("drain 后 activeEditMotion 应非 null（forSelectionMove 已创建）", motion)

        // sample(0L)：progress=0，caret 应在 origin（motion 用真实 frameTime 创建，未提前跳到 target）
        state.sampleVisualScene(0L)
        val layoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val originCaret = layoutSnapshot.cursorRect(0)
        val targetCaret = layoutSnapshot.cursorRect(1)
        assertNotEquals("origin/target caret 应不同（'ab' offset 0 与 offset 1 位置不同）", originCaret, targetCaret)
        assertEquals(
            "sample(0L) 时 caret 应在 origin（motion 用真实 frameTime 创建，progress=0）",
            originCaret,
            state.drawSnapshot().caretRect,
        )

        // sample(durationNanos)：progress=1，caret 应在 target
        state.sampleVisualScene(cursorDurationNanos)
        assertEquals(
            "sample(durationNanos) 时 caret 应在 target（progress=1）",
            targetCaret,
            state.drawSnapshot().caretRect,
        )
    }

    // ==================== 缺口3：handoff 不提前画 target caret ====================

    /**
     * 缺口3：publishLocalHandoffScene 不提前把 caret 推到 target。
     *
     * 场景：
     * 1. onAuthoritativeLayout("", selection=(0,0)) 设置基线
     * 2. recordLocalInput("" -> "a", oldSelection=(0,0), newSelection=(1,1))
     * 3. onAuthoritativeLayout("a", selection=(1,1)) 触发 publishLocalHandoffScene
     *
     * 修复后 handoffCaretRect = activeEditMotion?.let { drawSnapshotState.caretRect } ?: patch.originCaretRect。
     * 此时无 active motion，用 patch.originCaretRect（编辑前位置 = oldLayout.cursorRect(0)）。
     *
     * 验证：drawSnapshot().caretRect == oldLayout.cursorRect(0)（origin），!= newLayout.cursorRect(1)（target）。
     */
    @Test
    fun gap3_handoff_doesNotPreDrawTargetCaret() {
        val layouts = captureLayouts("", "a")
        val state =
            ComposeEditorVisualState(
                targetId = "issue728-5755336403-gap3",
                classifier = FakeLocalVisualPlanClassifier,
            )
        // 基线：空文本
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        // 插入 "a"
        state.recordLocalInput(
            oldText = "",
            newText = "a",
            oldSelection = TextRange(0, 0),
            newSelection = TextRange(1, 1),
            changes = listOf(LocalInputChange(newRange = TextRange(0, 1), oldRange = TextRange(0, 0))),
        )
        // onAuthoritativeLayout 触发 publishLocalHandoffScene
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0, compositionActive = false)

        val oldLayoutSnapshot = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val newLayoutSnapshot = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)
        val originCaret = oldLayoutSnapshot.cursorRect(0)
        val targetCaret = newLayoutSnapshot.cursorRect(1)
        assertNotEquals("origin/target caret 应不同", originCaret, targetCaret)

        val caretRect = state.drawSnapshot().caretRect
        assertNotNull("handoff 后 drawSnapshot().caretRect 应非 null", caretRect)
        assertEquals(
            "handoff 不应提前把 caret 推到 target，应保持 origin（patch.originCaretRect）",
            originCaret,
            caretRect,
        )
        assertFalse(
            "handoff caret 不应是 target（不应提前画编辑后位置）",
            targetCaret == caretRect,
        )
    }

    // ==================== 缺口4：Enter 按 text edit 处理 ====================

    /**
     * 缺口4：Enter（oldText != newText）走 text edit 分支，用 textDurationMillis 而非 cursorDurationMillis。
     *
     * 场景："a" -> "a\n"（Enter），oldText != newText。
     * 修复后 isSelectionOnly = (oldText == newText) && insertedKeys.isEmpty() && deletedKeys.isEmpty()，
     * Enter 的 oldText != newText → isSelectionOnly=false → 走 forEdit 分支，durationNanos = textDurationNanos。
     *
     * 旧实现用 insertedKeys.isEmpty() && deletedKeys.isEmpty() 判断 selection-only，
     * Enter 无 glyph unit 时误判为 selection-only，用 cursorDurationMillis。
     *
     * 验证：设置 textDurationMillis=200ms, cursorDurationMillis=50ms，
     *   motion 的 durationNanos == 200ms * NANOS_PER_MS（text edit 分支）。
     */
    @Test
    fun gap4_enterKey_treatedAsTextEdit_notSelectionOnly() {
        val layouts = captureLayouts("a", "a\n")
        val state =
            ComposeEditorVisualState(
                targetId = "issue728-5755336403-gap4",
                classifier = FakeLocalVisualPlanClassifier,
            )
        val textDurationMs = 200L
        val cursorDurationMs = 50L
        // 设置 textDuration != cursorDuration 以区分 forEdit / forSelectionMove 分支
        state.updateMotionPolicy(
            EditorMotionPolicy(
                textEnabled = true,
                textDurationMillis = textDurationMs,
                cursorEnabled = true,
                cursorDurationMillis = cursorDurationMs,
                coordinated = true,
            ),
        )
        // 基线："a"，caret 在 offset 1
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)
        // Enter：插入 "\n"
        state.recordLocalInput(
            oldText = "a",
            newText = "a\n",
            oldSelection = TextRange(1, 1),
            newSelection = TextRange(2, 2),
            changes = listOf(LocalInputChange(newRange = TextRange(1, 2), oldRange = TextRange(1, 1))),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0, compositionActive = false)
        // drain 创建 activeEditMotion
        state.drainPendingPatchesAtFrame(0L)

        val motion = stateField(state, "activeEditMotion")
        assertNotNull("Enter 编辑后 activeEditMotion 应非 null", motion)
        val durationNanos = motionDurationNanos(motion as ComposeEditMotion)
        assertEquals(
            "Enter（oldText != newText）应走 text edit 分支，durationNanos == textDurationMillis * NANOS_PER_MS",
            textDurationMs * NANOS_PER_MS,
            durationNanos,
        )
        assertFalse(
            "Enter 不应走 selection-only 分支（durationNanos 不应是 cursorDurationMillis * NANOS_PER_MS）",
            cursorDurationMs * NANOS_PER_MS == durationNanos,
        )
    }

    // ==================== 缺口5a：smooth cursor 读真实独立设置字段 ====================

    /**
     * 缺口5a：EditorSettingsOps.loadEditorSettingsSnapshot 的 smoothCursorEnabled 读
     * settings.editorSmoothCursorEnabled，smoothCursorDurationMs 读 settings.editorSmoothCursorDurationMs，
     * 不跟 typing animation 共用 editorTypingAnimationEnabled / editorTypingAnimationDurationMs。
     *
     * 源码级断言：读 EditorSettingsOps.kt 源码，验证 smoothCursor 赋值行引用独立字段。
     */
    @Test
    fun gap5a_loadEditorSettingsSnapshot_readsSmoothCursorFields() {
        // 候选路径覆盖不同工作目录（模块根 / apps/android / 仓库根）
        val relPath = "src/main/kotlin/com/xiwei/sujian/feature/editor/presentation/EditorSettingsOps.kt"
        val candidates =
            listOf(
                File(relPath),
                File("app/$relPath"),
                File("apps/android/app/$relPath"),
            )
        val sourceFile = candidates.firstOrNull { it.exists() }
        assumeTrue("EditorSettingsOps.kt 源码文件应存在（跳过：工作目录不含模块源码）", sourceFile != null)
        val source = sourceFile!!.readText()
        assertTrue(
            "smoothCursorEnabled 应引用 settings.editorSmoothCursorEnabled（独立 cursor 设置，非 typing animation）",
            source.contains("smoothCursorEnabled = settings.editorSmoothCursorEnabled"),
        )
        assertTrue(
            "smoothCursorDurationMs 应引用 settings.editorSmoothCursorDurationMs",
            source.contains("smoothCursorDurationMs = settings.editorSmoothCursorDurationMs"),
        )
        // 验证 smoothCursorEnabled 赋值行不引用 editorTypingAnimationEnabled（防止回退到共用字段）
        val smoothCursorEnabledLine = source.lineSequence().firstOrNull { it.contains("smoothCursorEnabled =") }
        assertNotNull("应有 smoothCursorEnabled 赋值行", smoothCursorEnabledLine)
        assertFalse(
            "smoothCursorEnabled 不应引用 editorTypingAnimationEnabled（缺口5a：读独立 cursor 设置）",
            smoothCursorEnabledLine!!.contains("editorTypingAnimationEnabled"),
        )
    }

    // ==================== 缺口5b：policy 切换保留 resting caret ====================

    /**
     * 缺口5b：updateMotionPolicy settle 动画但保留 resting caret。
     *
     * 场景：
     * 1. onAuthoritativeLayout("ab", selection=(1,1)) 设置 restingCaretRect
     * 2. sampleVisualScene(0L) 把 drawSnapshotState.caretRect 同步为 restingCaretRect
     * 3. updateMotionPolicy(newPolicy) 切换 policy
     *
     * 修复后 updateMotionPolicy 不清 restingCaretRect / drawSnapshotState.caretRect，
     * 只清 scene 和 activeEditMotion。
     *
     * 验证：drawSnapshot().caretRect 非 null 且等于切换前的值；restingCaretRect 非 null。
     */
    @Test
    fun gap5b_updateMotionPolicy_preservesRestingCaret() {
        val layouts = captureLayouts("ab")
        val state =
            ComposeEditorVisualState(
                targetId = "issue728-5755336403-gap5b",
                classifier = FakeLocalVisualPlanClassifier,
            )
        // 基线："ab"，selection=(1,1)，设置 restingCaretRect = cursorRect(1)
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)
        // sample 把 drawSnapshotState.caretRect 同步为 restingCaretRect（无 active motion）
        state.sampleVisualScene(0L)
        val caretBefore = state.drawSnapshot().caretRect
        assertNotNull("policy 切换前 caretRect 应非 null", caretBefore)

        // 切换 policy（reduceMotion=true，settle 所有动画）
        state.updateMotionPolicy(
            EditorMotionPolicy(
                textEnabled = false,
                textDurationMillis = 100L,
                cursorEnabled = false,
                cursorDurationMillis = 50L,
                coordinated = false,
                reduceMotion = true,
            ),
        )

        val caretAfter = state.drawSnapshot().caretRect
        assertNotNull("policy 切换后 caretRect 应保留（非 null，不消失）", caretAfter)
        assertEquals(
            "policy 切换应保留当前屏幕 caret 位置（不清 drawSnapshotState.caretRect）",
            caretBefore,
            caretAfter,
        )
        val restingCaretRect = stateField(state, "restingCaretRect")
        assertNotNull("restingCaretRect 应保留（非 null，policy 切换不清静止 caret）", restingCaretRect)
    }

    // ==================== 辅助方法 ====================

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L
    }

    /**
     * 反射读取 [ComposeEditorVisualState] 的私有字段值。
     *
     * 用于验证 activeEditMotion / restingCaretRect / lastResolvedText 等内部状态。
     */
    private fun stateField(
        state: ComposeEditorVisualState,
        name: String,
    ): Any? {
        val field = ComposeEditorVisualState::class.java.getDeclaredField(name)
        field.isAccessible = true
        return field.get(state)
    }

    /**
     * 反射读取 [ComposeEditMotion] 的私有 glyphDurationNanos 字段。
     *
     * 用于区分 text edit 分支（glyphDurationNanos = textDurationNanos）与
     * forSelectionMove 分支（glyphDurationNanos = 0）。
     * 注释 5754045689 重写后统一 motion 把单 duration 拆成 caretDurationNanos / glyphDurationNanos，
     * 文字吞吐走 glyphDurationNanos，所以这里读 glyphDurationNanos。
     */
    private fun motionDurationNanos(motion: ComposeEditMotion): Long {
        val field = ComposeEditMotion::class.java.getDeclaredField("glyphDurationNanos")
        field.isAccessible = true
        return field.get(motion) as Long
    }

    /**
     * 用真实 TextMeasurer 构造 [TextLayoutResult]（cursorRect 需要 layoutInput 几何）。
     */
    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = 14f.sp),
                        constraints = Constraints(maxWidth = 1000),
                    ),
                )
            }
        }
        return results
    }
}
