package com.xiwei.sujian.arch

import com.xiwei.sujian.editor.v2.coordinator.EditorWindowHost
import com.xiwei.sujian.editor.v2.projection.ChapterPreviewState
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 九：预览用纯静态 ChapterPreviewState 结构契约测试（#597 移入独立架构检查集合）。
 *
 * 验证：
 * - ChapterPreviewState 不持有动画引擎/Bitmap/VisualRuntime 相关字段；
 * - EditorWindowHost 提供 getChapterPreviewState(String): ChapterPreviewState?。
 */
class ChapterPreviewStateArchitectureTest {
    @Test
    fun chapterPreviewStateHasNoAnimationFields() {
        val fields = ChapterPreviewState::class.java.declaredFields
        val fieldNames = fields.map { it.name }
        assertTrue("Must have text field", fieldNames.contains("text"))
        assertTrue("Must have revision field", fieldNames.contains("revision"))
        assertFalse(
            "Must not have animationEngine field",
            fieldNames.any { it.contains("animation") || it.contains("Animation") },
        )
        assertFalse(
            "Must not have visualRuntime field",
            fieldNames.any { it.contains("visualRuntime") || it.contains("VisualRuntime") },
        )
        assertFalse(
            "Must not have bitmap field",
            fieldNames.any { it.contains("bitmap") || it.contains("Bitmap") },
        )
    }

    @Test
    fun getChapterPreviewStateExistsOnEditorWindowHost() {
        val method =
            EditorWindowHost::class.java.methods.firstOrNull {
                it.name == "getChapterPreviewState" &&
                    it.parameterTypes.size == 1 &&
                    it.parameterTypes[0] == String::class.java &&
                    it.returnType == ChapterPreviewState::class.java
            }
        assertNotNull("EditorWindowHost must have getChapterPreviewState(String): ChapterPreviewState?", method)
    }

    private fun assertFalse(
        message: String,
        condition: Boolean,
    ) {
        org.junit.Assert.assertFalse(message, condition)
    }
}
