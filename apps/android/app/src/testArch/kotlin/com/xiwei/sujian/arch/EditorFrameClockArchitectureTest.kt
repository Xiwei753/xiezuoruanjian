package com.xiwei.sujian.arch

import com.xiwei.sujian.editor.v2.coordinator.EditorSessionCoordinator
import com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost
import com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #595 七：单一 VSync 帧驱动架构约束测试 — 静态结构检查。
 *
 * 验证活动编辑时 SujianEditorView 是唯一的 FrameClock listener，投影运行时不接到
 * FrameClock（避免双驱动）。这些是类/字段/方法存在性约束，属于架构层结构检查，
 * 不是运行时行为测试（运行时行为由 SingleFrameDriveTest 用假时钟驱动验证）。
 *
 * - EditorWindowHost 拥有唯一的 WindowDisplayFrameClock 字段；
 * - EditorSessionCoordinator 不拥有 WindowDisplayFrameClock（会话层只持纯数据）；
 * - EditorWindowHost 暴露 beginEdit / releaseWindow 生命周期入口。
 */
class EditorFrameClockArchitectureTest {
    @Test
    fun windowHost_ownsSingleFrameClock() {
        val field =
            EditorWindowHost::class.java.declaredFields.firstOrNull {
                it.name == "windowFrameClock"
            }
        assertNotNull(
            "EditorWindowHost must own a single WindowDisplayFrameClock",
            field,
        )
        assertTrue(
            "windowFrameClock must be a WindowDisplayFrameClock",
            field != null && field.type == WindowDisplayFrameClock::class.java,
        )
    }

    @Test
    fun sessionCoordinator_doesNotOwnFrameClock() {
        val hasFrameClock =
            EditorSessionCoordinator::class.java.declaredFields.any {
                it.type == WindowDisplayFrameClock::class.java
            }
        assertTrue(
            "EditorSessionCoordinator must NOT own a WindowDisplayFrameClock " +
                "(session layer holds only pure data; FrameClock is window-owned)",
            !hasFrameClock,
        )
    }

    @Test
    fun beginEditExistsOnWindowHost() {
        val method: Method? =
            EditorWindowHost::class.java.methods.firstOrNull {
                it.name == "beginEdit"
            }
        assertNotNull(
            "EditorWindowHost must have beginEdit for active editing",
            method,
        )
    }

    @Test
    fun releaseWindowDisconnectsFrameClock() {
        val method: Method? =
            EditorWindowHost::class.java.methods.firstOrNull {
                it.name == "releaseWindow"
            }
        assertNotNull(
            "EditorWindowHost must have releaseWindow to disconnect FrameClock on config change",
            method,
        )
    }
}
