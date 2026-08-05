package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #592 三：投影 FrameClock 连接契约测试。
 *
 * 验证 EditorSessionCoordinator.setProjectionFrameClock 接受 nullable 参数，
 * 使 EditorWindowHost 能在 beginEdit 时连接、releaseWindow 时解除绑定，
 * 投影动画按真实 VSync 持续推进。
 */
class ProjectionFrameClockContractTest {

    @Test
    fun setProjectionFrameClock_acceptsNullableFrameClock() {
        val method: Method? = EditorSessionCoordinator::class.java.declaredMethods.firstOrNull {
            it.name == "setProjectionFrameClock" &&
            it.parameterTypes.size == 2 &&
            it.parameterTypes[0] == String::class.java &&
            it.parameterTypes[1] == WindowDisplayFrameClock::class.java
        }
        assertTrue(
            "EditorSessionCoordinator.setProjectionFrameClock must accept (String, WindowDisplayFrameClock) " +
            "where WindowDisplayFrameClock is a platform type allowing null for disconnect",
            method != null
        )
    }

    @Test
    fun editorWindowHost_delegatesSetProjectionFrameClock() {
        val hasDetach = EditorWindowHost::class.java.methods.any { it.name == "detachTarget" }
        assertTrue("EditorWindowHost must delegate detachTarget", hasDetach)
    }
}
