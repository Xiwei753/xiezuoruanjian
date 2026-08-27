package com.xiwei.sujian.feature.project.ui

import org.junit.Assert.assertEquals
import org.junit.Test

class PreparedEditorTargetTest {
    @Test
    fun targetId_isStableForSameChapterAndChangesForDifferentChapter() {
        val first = PreparedEditorTarget("project", "volume", "chapter-a", "A")
        val sameChapterWithUpdatedTitle = first.copy(chapterTitle = "Updated A")
        val second = first.copy(chapterId = "chapter-b")

        assertEquals(first.targetId, sameChapterWithUpdatedTitle.targetId)
        assertEquals("chapter-body:project:volume:chapter-a", first.targetId)
        assertEquals("chapter-body:project:volume:chapter-b", second.targetId)
    }
}
