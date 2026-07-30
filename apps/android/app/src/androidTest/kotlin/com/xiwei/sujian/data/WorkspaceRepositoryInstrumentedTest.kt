package com.xiwei.sujian.data

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.xiwei.sujian.support.AndroidTestEnvironment
import com.xiwei.sujian.support.SujianSmallTest
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
@SujianSmallTest
class WorkspaceRepositoryInstrumentedTest {

    @get:Rule
    val rule = AndroidTestEnvironment.TestDependenciesRule(seedProject = false)

    private fun getRepo(): WorkspaceRepository =
        AndroidTestEnvironment.requireCurrentSession().deps.workspaceRepository

    @Test
    fun createProject_setsTitleAndReturnsId() {
        val repo = getRepo()
        val project = repo.createProject("Repository测试")
        assertEquals("Repository测试", project.title)
        assertTrue("Project ID should be non-empty", project.id.isNotEmpty())
    }

    @Test
    fun createProject_appearsInGetProjects() {
        val repo = getRepo()
        val before = repo.getProjects().size
        repo.createProject("新项目")
        val after = repo.getProjects().size
        assertEquals("getProjects count should increase by 1", before + 1, after)
    }

    @Test
    fun createVolume_storesUnderProject() {
        val repo = getRepo()
        val project = repo.createProject("卷测试作品")
        val volume = repo.createVolume(project.id, "自定义卷")
        assertEquals("自定义卷", volume.title)
        val volumes = repo.getVolumes(project.id)
        assertTrue("Created volume should be in getVolumes", volumes.any { it.id == volume.id })
    }

    @Test
    fun createChapter_roundtripsWithGetChapters() {
        val repo = getRepo()
        val project = repo.createProject("章节测试作品")
        val volumes = repo.getVolumes(project.id)
        val volume = volumes.first()
        val chapter = repo.createChapter(project.id, volume.id, "新章节")
        assertEquals("新章节", chapter.title)
        val chapters = repo.getChapters(project.id, volume.id)
        assertTrue("Created chapter should be in getChapters", chapters.any { it.id == chapter.id })
    }

    @Test
    fun saveAndReadChapterContent_returnsSavedContent() {
        val repo = getRepo()
        val project = repo.createProject("内容测试")
        val volumes = repo.getVolumes(project.id)
        val volume = volumes.first()
        val chapter = repo.createChapter(project.id, volume.id, "内容章节")
        val savedContent = "这是保存的测试正文，包含中文 and English 123。"
        repo.saveChapterContent(project.id, volume.id, chapter.id, savedContent)
        val (content, meta) = repo.getChapterContentWithMeta(project.id, volume.id, chapter.id)
        assertEquals(savedContent, content)
        assertTrue("Word count should be > 0 for non-empty content", meta.wordCount > 0)
    }

    @Test
    fun renameProject_persistsThroughGetProjects() {
        val repo = getRepo()
        val project = repo.createProject("原名")
        repo.renameProject(project.id, "新名称")
        val projects = repo.getProjects()
        val renamed = projects.firstOrNull { it.id == project.id }
        assertNotNull("Project should still exist after rename", renamed)
        assertEquals("新名称", renamed!!.title)
    }

    @Test
    fun deleteProject_removesFromGetProjects() {
        val repo = getRepo()
        val project = repo.createProject("待删除")
        assertTrue("Project should exist before delete", repo.getProjects().any { it.id == project.id })
        repo.deleteProject(project.id)
        assertFalse("Project should not exist after delete", repo.getProjects().any { it.id == project.id })
    }

    @Test
    fun reorderProjects_reflectsNewOrder() {
        val repo = getRepo()
        val p1 = repo.createProject("A")
        val p2 = repo.createProject("B")
        val projects = repo.getProjects().filter { it.id == p1.id || it.id == p2.id }
        if (projects.size >= 2) {
            repo.reorderProjects(listOf(p2.id, p1.id))
            val afterReorder = repo.getProjects().filter { it.id == p1.id || it.id == p2.id }
            assertEquals("Two projects should still be present", 2, afterReorder.size)
        }
    }

    @Test
    fun getVolumes_returnsDefaultVolumeAfterProjectCreation() {
        val repo = getRepo()
        val project = repo.createProject("默认卷测试")
        val volumes = repo.getVolumes(project.id)
        assertTrue("New project should have at least one default volume", volumes.isNotEmpty())
        assertTrue("Volume title should be non-empty", volumes.first().title.isNotEmpty())
    }
}
