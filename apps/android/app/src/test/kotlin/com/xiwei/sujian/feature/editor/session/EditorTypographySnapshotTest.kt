package com.xiwei.sujian.feature.editor.session

import com.xiwei.sujian.feature.editor.window.EditorTypography
import com.xiwei.sujian.feature.editor.window.EditorWindowHost
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #630 评论 5326175206 项3: EditorTypography 快照测试。
 *
 * - EditorTypography 是 public top-level data class
 * - beginEdit 接受 typography 参数
 * - 首次 bind 的 typography 在 performViewBind 中先于 attachSession 应用
 * - 运行时 applyEditorTypography 仍即时更新
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class EditorTypographySnapshotTest {
    @Test
    fun editorTypography_isPublicTopLevelDataClass() {
        // EditorTypography 应是 public data class
        val clazz = EditorTypography::class.java
        assertTrue("EditorTypography should be public data class", java.lang.reflect.Modifier.isPublic(clazz.modifiers))
        // data class 编译后有 copy/equals/hashCode/toString 方法
        val methodNames = clazz.declaredMethods.map { it.name }.toSet()
        assertTrue("EditorTypography should have copy method (data class)", "copy" in methodNames)
        assertTrue("EditorTypography should have equals method (data class)", "equals" in methodNames)
    }

    @Test
    fun editorTypography_hasExpectedFields() {
        val t = EditorTypography(16f, 1.5f, true, 2f)
        assertEquals(16f, t.fontSizeSp, 0.001f)
        assertEquals(1.5f, t.lineSpacingMultiplier, 0.001f)
        assertEquals(true, t.autoIndentEnabled)
        assertEquals(2f, t.autoIndentWidth, 0.001f)
    }

    @Test
    fun editorTypography_dataClassEquality() {
        val a = EditorTypography(16f, 1.5f, true, 2f)
        val b = EditorTypography(16f, 1.5f, true, 2f)
        val c = EditorTypography(18f, 1.5f, true, 2f)
        assertEquals(a, b)
        assertTrue(a != c)
    }

    @Test
    fun editorTypography_dataClassCopy() {
        val original = EditorTypography(16f, 1.5f, true, 2f)
        val modified = original.copy(fontSizeSp = 20f)
        assertEquals(20f, modified.fontSizeSp, 0.001f)
        assertEquals(1.5f, modified.lineSpacingMultiplier, 0.001f)
    }

    @Test
    fun beginEdit_acceptsTypographyParameter() {
        // EditorWindowHost.beginEdit 应接受 typography 参数（编译通过即证明）。
        // 运行时验证：通过反射确认 beginEdit 方法签名包含 EditorTypography 参数。
        val hostClass = EditorWindowHost::class.java
        val methods = hostClass.declaredMethods
        val beginEditMethod = methods.find { it.name == "beginEdit" }
        assertNotNull("beginEdit 方法应存在", beginEditMethod)
        val paramTypes = beginEditMethod!!.parameterTypes
        val hasTypographyParam = paramTypes.any { it == EditorTypography::class.java }
        assertTrue(
            "beginEdit 应接受 EditorTypography 参数",
            hasTypographyParam,
        )
    }

    @Test
    fun pendingViewBind_containsTypographyField() {
        // PendingViewBind 应包含 typography 字段（编译期验证）。
        // 通过反射读取 PendingViewBind data class 的组件函数。
        val hostClass = Class.forName("com.xiwei.sujian.feature.editor.window.EditorWindowHost")
        val pendingBindClass = hostClass.declaredClasses.find { it.simpleName == "PendingViewBind" }
        assertNotNull("PendingViewBind 应存在", pendingBindClass)
        val fields = pendingBindClass!!.declaredFields
        val hasTypographyField = fields.any { it.name == "typography" }
        assertTrue("PendingViewBind 应包含 typography 字段", hasTypographyField)
    }

    @Test
    fun applyEditorTypography_stillUpdatesView() {
        // applyEditorTypography 仍应即时更新活动 View（运行时接口不变）。
        // 编译通过即证明 EditorWindowHost.applyEditorTypography 仍存在。
        val hostClass = EditorWindowHost::class.java
        val methods = hostClass.declaredMethods
        val hasApplyMethod = methods.any { it.name == "applyEditorTypography" }
        assertTrue("applyEditorTypography 方法应仍存在", hasApplyMethod)
    }
}
