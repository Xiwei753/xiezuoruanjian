package com.xiwei.sujian.arch

import com.xiwei.sujian.editor.v2.pipeline.FrameRenderInput
import com.xiwei.sujian.editor.v2.visual.PreparedVisualTransaction
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Field

/**
 * #595 五：静态文字路径 + 动画光标渲染结构契约测试（#597 移入独立架构检查集合）。
 *
 * 验证 FrameRenderInput 携带 cursorTransition 字段且与文字事务解耦。
 */
class StaticTextAnimatedCursorArchitectureTest {
    @Test
    fun frameRenderInputCarriesCursorTransitionField() {
        val field: Field? =
            FrameRenderInput::class.java.declaredFields.firstOrNull {
                it.name == "cursorTransition"
            }
        assertNotNull(
            "FrameRenderInput 必须携带 cursorTransition 字段 — " +
                "文字轨结束后静态文字路径依赖它绘制平滑光标",
            field,
        )
        assertTrue(
            "cursorTransition 必须与文字事务解耦（可独立为 null）",
            field!!.type == PreparedVisualTransaction.CursorTransition::class.java ||
                field.type.kotlin.javaObjectType == PreparedVisualTransaction.CursorTransition::class.java,
        )
    }
}
