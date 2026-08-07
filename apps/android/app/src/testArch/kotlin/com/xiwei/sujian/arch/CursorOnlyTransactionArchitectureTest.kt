package com.xiwei.sujian.arch

import com.xiwei.sujian.editor.v2.visual.AndroidTextAnimationEngine
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import org.junit.Assert.assertTrue
import org.junit.Test
import java.lang.reflect.Method

/**
 * #595 四：CursorOnly 事务结构契约测试（#597 移入独立架构检查集合）。
 *
 * 验证方法存在性：
 * - AndroidTextAnimationEngine 提供 submitCursorOnlyTransaction；
 * - SujianEditorView 提供 setKernelAnimationEnabled(Boolean)。
 */
class CursorOnlyTransactionArchitectureTest {

    @Test
    fun submitCursorOnlyTransactionMethodExists() {
        val method: Method? = AndroidTextAnimationEngine::class.java.declaredMethods.firstOrNull {
            it.name == "submitCursorOnlyTransaction"
        }
        assertTrue(
            "AndroidTextAnimationEngine must have submitCursorOnlyTransaction for CursorOnly path",
            method != null,
        )
    }

    @Test
    fun setKernelAnimationEnabledExistsOnSujianEditorView() {
        val method: Method? = SujianEditorView::class.java.methods.firstOrNull {
            it.name == "setKernelAnimationEnabled" &&
            it.parameterTypes.size == 1 &&
            it.parameterTypes[0] == Boolean::class.javaPrimitiveType
        }
        assertTrue(
            "SujianEditorView must have setKernelAnimationEnabled(Boolean) " +
            "to decouple kernel animation_enabled from text-only suppression",
            method != null,
        )
    }
}
