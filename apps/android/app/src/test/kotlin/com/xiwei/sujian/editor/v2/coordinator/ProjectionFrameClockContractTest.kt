package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #592 三/四 + #595 九：投影运行时归属与第二套运行时删除契约测试。
 *
 * - 窗口销毁时由窗口层释放 FrameClock；会话层不持有 TargetDisplayRuntime/FrameClock。
 * - #595 九：EditorWindowHost 不再创建/持有 TargetDisplayRuntime 第二套动画运行时 —
 *   非活动预览只使用会话层 snapshot 派生的纯静态 ChapterPreviewState。
 */
class ProjectionFrameClockContractTest {

    @Test
    fun editorWindowHost_releaseWindowExists() {
        val method: Method? = EditorWindowHost::class.java.methods.firstOrNull {
            it.name == "releaseWindow"
        }
        assertTrue("EditorWindowHost must have releaseWindow()", method != null)
    }

    @Test
    fun sessionCoordinator_doesNotExposeSetProjectionFrameClock() {
        // #592 四：FrameClock 绑定是窗口层职责，会话层不再暴露该入口。
        val method = EditorSessionCoordinator::class.java.methods.firstOrNull {
            it.name == "setProjectionFrameClock"
        }
        assertTrue(
            "EditorSessionCoordinator must NOT own setProjectionFrameClock after #592 四 " +
            "(TargetDisplayRuntime/FrameClock moved to EditorWindowHost)",
            method == null
        )
    }

    @Test
    fun sessionCoordinator_holdsOnlyPureSessionState() {
        val hasTargetsField = EditorSessionCoordinator::class.java.declaredFields.any {
            it.name == "targets"
        }
        assertTrue(
            "EditorSessionCoordinator must not hold EditableTextTarget map after #592 四 " +
            "(window objects moved to EditorWindowHost)",
            !hasTargetsField
        )
        val hasProjectionsField = EditorSessionCoordinator::class.java.declaredFields.any {
            it.name == "targetProjections"
        }
        assertTrue(
            "EditorSessionCoordinator must not hold TargetDisplayRuntime map after #592 四",
            !hasProjectionsField
        )
    }

    @Test
    fun editorWindowHost_noSecondAnimationRuntime() {
        // #595 九：预览纯静态化后，EditorWindowHost 不得再持有 TargetDisplayRuntime 第二套运行时。
        val hasProjectionsField = EditorWindowHost::class.java.declaredFields.any {
            it.name == "targetProjections"
        }
        assertTrue(
            "EditorWindowHost must NOT hold TargetDisplayRuntime map after #595 九 " +
            "(preview is pure static ChapterPreviewState)",
            !hasProjectionsField
        )
        val runtimeField = EditorWindowHost::class.java.declaredFields.any {
            it.type.simpleName == "TargetDisplayRuntime"
        }
        assertTrue(
            "EditorWindowHost must not hold any TargetDisplayRuntime field after #595 九",
            !runtimeField
        )
    }
}
