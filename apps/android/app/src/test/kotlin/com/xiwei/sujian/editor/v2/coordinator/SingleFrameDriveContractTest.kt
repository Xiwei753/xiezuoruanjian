package com.xiwei.sujian.editor.v2.coordinator

import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #595 七：单一 VSync 帧驱动契约测试 — 验证活动编辑时 SujianEditorView 是唯一的
 * FrameClock listener，投影运行时不接到 FrameClock（避免双驱动）。
 *
 * beginEdit 不再为活动 target 调用 targetProjections[targetId]?.setFrameClock(windowFrameClock)；
 * 投影只在非活动预览（ReadonlyChapterPreview）时静态绘制。
 */
class SingleFrameDriveContractTest {

    @Test
    fun windowHost_ownsSingleFrameClock() {
        val field = EditorWindowHost::class.java.declaredFields.firstOrNull {
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
        val hasFrameClock = EditorSessionCoordinator::class.java.declaredFields.any {
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
        val method: Method? = EditorWindowHost::class.java.methods.firstOrNull {
            it.name == "beginEdit"
        }
        assertNotNull(
            "EditorWindowHost must have beginEdit for active editing",
            method,
        )
    }

    @Test
    fun releaseWindowDisconnectsFrameClock() {
        val method: Method? = EditorWindowHost::class.java.methods.firstOrNull {
            it.name == "releaseWindow"
        }
        assertNotNull(
            "EditorWindowHost must have releaseWindow to disconnect FrameClock on config change",
            method,
        )
    }
}
