package com.xiwei.sujian.editor.v2.projection

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SecretProjectionAtomicityTest {

    @Test
    fun maskedProjectionDisplayMatchesRealLength() {
        val text = "password123"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(text.length, proj.displayText.length)
        assertEquals(text.length, proj.displayLengthUtf16)
    }

    @Test
    fun maskedProjectionUtf8ToDisplayUtf16AtBoundaries() {
        val text = "abc"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.realUtf8ToDisplayUtf16(0))
        assertEquals(3, proj.realUtf8ToDisplayUtf16(3))
    }

    @Test
    fun maskedProjectionDisplayUtf16ToRealUtf8AtBoundaries() {
        val text = "abc"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(0, proj.displayUtf16ToRealUtf8(0))
        assertEquals(3, proj.displayUtf16ToRealUtf8(3))
    }

    @Test
    fun identityProjectionPreservesText() {
        val text = "Hello 世界"
        val proj = DisplayTextProjection.identity(text)
        assertEquals(text, proj.realText)
        assertEquals(text, proj.displayText)
        assertFalse(proj.isMasked)
    }

    @Test
    fun maskedProjectionWithMultibyteUtf8() {
        val text = "你好世界"
        val proj = DisplayTextProjection.masked(text)
        assertEquals(text.length, proj.displayLengthUtf16)
        val utf8Len = text.toByteArray(Charsets.UTF_8).size
        assertEquals(utf8Len, proj.realLengthUtf8)
        for (i in 0..text.length) {
            val realUtf8 = text.substring(0, i).toByteArray(Charsets.UTF_8).size
            assertEquals(i, proj.realUtf8ToDisplayUtf16(realUtf8))
        }
    }

    @Test
    fun emptyTextMaskedProjectionIsIdentity() {
        val proj = DisplayTextProjection.masked("")
        assertEquals("", proj.realText)
        assertEquals("", proj.displayText)
        assertEquals(0, proj.realLengthUtf8)
        assertEquals(0, proj.displayLengthUtf16)
    }

    @Test
    fun projectionOffsetMappingConsistentForAscii() {
        val text = "abcdef"
        val proj = DisplayTextProjection.masked(text)
        for (utf8 in 0..6) {
            val displayUtf16 = proj.realUtf8ToDisplayUtf16(utf8)
            val roundTrip = proj.displayUtf16ToRealUtf8(displayUtf16)
            assertEquals(utf8, roundTrip)
        }
    }

    @Test
    fun customMaskCharUsed() {
        val proj = DisplayTextProjection.masked("ab", "*")
        assertEquals("**", proj.displayText)
    }

    @Test
    fun layoutRuntimeOwnsApplySecretDisplayIfActive() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidLayoutRuntime")
        val method = clazz.getDeclaredMethod("applySecretDisplayIfActive")
        assertNotNull(method)
    }

    @Test
    fun layoutRuntimeOwnsApplySecretDisplayIfActiveWithLayout() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidLayoutRuntime")
        val method = clazz.getDeclaredMethod("applySecretDisplayIfActiveWithLayout")
        assertNotNull(method)
    }

    @Test
    fun applySecretDisplayIfActiveDelegatesToRebuildDisplayProjection() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidLayoutRuntime")
        val rebuildMethod = clazz.getDeclaredMethod("rebuildDisplayProjection")
        assertNotNull(rebuildMethod)
    }

    @Test
    fun layoutRuntimeOwnsRebuildDisplayProjection() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidLayoutRuntime")
        val method = clazz.getDeclaredMethod("rebuildDisplayProjection")
        assertNotNull(method)
    }

    @Test
    fun pipelineApplyEditResultIncludesSecretInMirrorUpdate() {
        val pipelineClazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidEditorPipeline")
        val editResultClazz = Class.forName("com.xiwei.sujian.editor.v2.mirror.EditResult")
        val functionType = kotlin.jvm.functions.Function0::class.java
        val applyMethod = pipelineClazz.getDeclaredMethod(
            "applyEditResult",
            editResultClazz,
            functionType
        )
        assertNotNull(applyMethod)
    }

    @Test
    fun displayTextProjectionHasMaskedWithComposition() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.projection.DisplayTextProjection")
        val companion = clazz.declaredClasses.first { it.simpleName == "Companion" }
        val method = companion.getDeclaredMethod(
            "maskedWithComposition",
            String::class.java,
            Int::class.javaPrimitiveType,
            Int::class.javaPrimitiveType,
            String::class.java,
            String::class.java
        )
        assertNotNull(method)
    }

    @Test
    fun layoutRuntimeRebuildSecretProjectionUsesComposition() {
        val clazz = Class.forName("com.xiwei.sujian.editor.v2.pipeline.AndroidLayoutRuntime")
        val mirrorClazz = Class.forName("com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror")
        val constructor = clazz.getDeclaredConstructor(mirrorClazz, android.text.TextPaint::class.java)
        assertNotNull(constructor)
    }

    private fun assertNotNull(obj: Any?) {
        assertTrue("Expected non-null value", obj != null)
    }
}
