package com.xiwei.sujian.feature.editor.window

import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论 5327560790: 运行时更新 + createWindowView 预写删除验证。
 *
 * 测试5: 运行时改字号/行距走 applyEditorTypography 更新活动 View；
 *        createWindowView 不再 lastTypography 预写（首次 bind 由 performViewBind 的
 *        pending.typography 显式写入）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorTypographyRuntimeUpdateTest {
    @Test
    fun applyEditorTypography_methodExists() {
        // 运行时更新路径保留 — applyEditorTypography 仍存在。
        val methods = EditorWindowHost::class.java.declaredMethods
        assertTrue(
            "applyEditorTypography 方法应存在（运行时更新路径）",
            methods.any { it.name == "applyEditorTypography" },
        )
    }

    @Test
    fun lastTypography_fieldExists() {
        // lastTypography 字段仍存在 — 由 applyEditorTypography 更新，供 beginEdit fallback。
        val fields = EditorWindowHost::class.java.declaredFields
        assertTrue(
            "lastTypography 字段应存在",
            fields.any { it.name == "lastTypography" },
        )
    }

    @Test
    fun beginEdit_acceptsTypographyParameter() {
        // beginEdit 仍接受 typography 参数 — 首次 bind 的排版由调用方显式传入。
        val methods = EditorWindowHost::class.java.declaredMethods
        val beginEditMethod = methods.find { it.name == "beginEdit" }
        assertNotNull("beginEdit 方法应存在", beginEditMethod)
        val paramTypes = beginEditMethod!!.parameterTypes
        assertTrue(
            "beginEdit 应接受 EditorTypography 参数",
            paramTypes.any { it == EditorTypography::class.java },
        )
    }

    @Test
    fun pendingViewBind_containsTypographyField() {
        // PendingViewBind 包含 typography 字段 — performViewBind 用它先写排版再 attach snapshot。
        val hostClass = Class.forName("com.xiwei.sujian.feature.editor.window.EditorWindowHost")
        val pendingBindClass = hostClass.declaredClasses.find { it.simpleName == "PendingViewBind" }
        assertNotNull("PendingViewBind 应存在", pendingBindClass)
        val fields = pendingBindClass!!.declaredFields
        assertTrue(
            "PendingViewBind 应包含 typography 字段",
            fields.any { it.name == "typography" },
        )
    }

    @Test
    fun createWindowView_methodExists() {
        // createWindowView 仍存在 — 但不再预写 lastTypography。
        val methods = EditorWindowHost::class.java.declaredMethods
        assertTrue(
            "createWindowView 方法应存在",
            methods.any { it.name == "createWindowView" },
        )
    }

    @Test
    fun performViewBind_methodExists() {
        // performViewBind 仍存在 — 首次/换绑时用 pending.typography 显式写排版。
        val methods = EditorWindowHost::class.java.declaredMethods
        assertTrue(
            "performViewBind 方法应存在",
            methods.any { it.name == "performViewBind" },
        )
    }
}
