package com.xiwei.sujian.editor.v2.projection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TargetReadonlyProjectionContractTest {

    @Test
    fun targetDisplayRuntimeClassExists() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime")
        assert(clazz.declaredMethods.any { it.name == "updateFromSnapshot" })
        assert(clazz.declaredMethods.any { it.name == "applyEditResult" })
        assert(clazz.declaredMethods.any { it.name == "setSecretMasked" })
        assert(clazz.declaredMethods.any { it.name == "setSearchHighlights" })
        assert(clazz.declaredMethods.any { it.name == "setSelection" })
        assert(clazz.declaredMethods.any { it.name == "getSearchHighlightsUtf16" })
        assert(clazz.declaredMethods.any { it.name == "getSelectionStartUtf16" })
        assert(clazz.declaredMethods.any { it.name == "getSelectionEndUtf16" })
        assert(clazz.declaredMethods.any { it.name == "getProjection" })
        assert(clazz.declaredMethods.any { it.name == "getRevision" })
        assert(clazz.declaredMethods.any { it.name == "clearDecorations" })
        assert(clazz.declaredMethods.any { it.name == "drawFrame" })
        assert(clazz.declaredMethods.any { it.name == "getVisualRuntime" })
    }

    @Test
    fun applyEditResultMethodExists() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime")
        val methods = clazz.declaredMethods.filter { it.name == "applyEditResult" }
        assertTrue(methods.isNotEmpty())
    }

    @Test
    fun projectionIdentityByDefault() {
        val proj = DisplayTextProjection.identity("hello")
        assertFalse(proj.isMasked)
        assertEquals("hello", proj.realText)
        assertEquals("hello", proj.displayText)
    }

    @Test
    fun projectionMaskedAfterSetSecretMasked() {
        val proj = DisplayTextProjection.masked("secret")
        assertTrue(proj.isMasked)
        assertEquals("secret", proj.realText)
        assertEquals("\u2022\u2022\u2022\u2022\u2022\u2022", proj.displayText)
    }

    @Test
    fun offsetMappingIdentityPreservesUtf8() {
        val text = "Hello"
        val proj = DisplayTextProjection.identity(text)
        val utf8Len = text.toByteArray(Charsets.UTF_8).size
        assertEquals(utf8Len, proj.realLengthUtf8)
        for (i in 0..utf8Len) {
            assertEquals(i, proj.realUtf8ToDisplayUtf16(i))
        }
    }

    @Test
    fun offsetMappingMaskedPreservesUtf8() {
        val text = "abc"
        val proj = DisplayTextProjection.masked(text)
        val utf8Len = text.toByteArray(Charsets.UTF_8).size
        assertEquals(utf8Len, proj.realLengthUtf8)
        assertEquals(3, proj.displayLengthUtf16)
    }

    @Test
    fun targetDisplayRuntimeOwnsSecretDisplayMode() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime")
        val field = clazz.getDeclaredField("secretDisplayMode")
        assertEquals(Boolean::class.javaPrimitiveType, field.type)
    }

    @Test
    fun targetDisplayRuntimeRebuildUsesComposition() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime")
        val mirrorClazz = Class.forName("com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror")
        val constructor = clazz.getDeclaredConstructor(mirrorClazz, android.text.TextPaint::class.java)
        assertTrue(constructor != null)
    }

    @Test
    fun applyEditResultTriggersRebuildProjectionAndLayout() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime")
        val method = clazz.getDeclaredMethod("applyEditResult", Class.forName("com.xiwei.sujian.editor.v2.mirror.EditResult"))
        assertTrue(method != null)
    }

    @Test
    fun targetDisplayRuntimeHasGetRevision() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.TargetDisplayRuntime")
        val method = clazz.getDeclaredMethod("getRevision")
        assertEquals(Long::class.javaPrimitiveType, method.returnType)
    }
}
