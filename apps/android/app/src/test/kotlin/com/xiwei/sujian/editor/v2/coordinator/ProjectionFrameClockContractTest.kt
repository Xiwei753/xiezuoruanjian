package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #592 三/四：投影 FrameClock 归属窗口层契约测试。
 *
 * 窗口销毁时由窗口层解除投影 FrameClock 绑定（targetProjections 属于
 * EditorWindowHost），避免已释放时钟继续驱动投影；会话层不再持有
 * TargetDisplayRuntime/FrameClock。
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
    fun editorWindowHost_ownsProjectionRuntimes() {
        val hasProjectionsField = EditorWindowHost::class.java.declaredFields.any {
            it.name == "targetProjections"
        }
        assertTrue(
            "EditorWindowHost must own TargetDisplayRuntime map after #592 四",
            hasProjectionsField
        )
    }
}
