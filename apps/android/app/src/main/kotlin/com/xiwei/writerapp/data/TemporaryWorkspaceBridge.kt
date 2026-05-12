package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.*
import android.content.Context
import com.google.gson.Gson
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.UUID

// TODO: Temporary Bridge. This class MUST be replaced with JNI calls to core/writer_core/src/facade.rs.
// Android is strictly forbidden from parsing or managing the workspace format directly.
class TemporaryWorkspaceBridge(private val context: Context) {
    private val gson = Gson()
    private val workspaceDir = WorkspaceManager.getWorkspaceDir(context)

    init {
        WorkspaceManager.initWorkspaceIfNeeded(context)
    }

    private fun getCurrentTimeStr(): String {
        val sdf = SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss'Z'", Locale.US)
        return sdf.format(Date())
    }

    fun getManifest(): WorkspaceManifest? {
        val manifestFile = File(workspaceDir, "workspace_manifest.json")
        if (!manifestFile.exists()) return null
        return try {
            gson.fromJson(manifestFile.readText(), WorkspaceManifest::class.java)
        } catch (e: Exception) {
            e.printStackTrace()
            null
        }
    }

    fun getLocalSettings(): LocalSettings? {
        val settingsFile = File(workspaceDir, "app-meta/settings/settings.local.json")
        if (!settingsFile.exists()) return null
        return try {
            gson.fromJson(settingsFile.readText(), LocalSettings::class.java)
        } catch (e: Exception) {
            e.printStackTrace()
            null
        }
    }

    fun getProjects(): List<Project> {
        val manifest = getManifest()
        if (manifest == null || manifest.version < 1) {
            return emptyList()
        }

        val projectsDir = File(workspaceDir, "projects")
        val projectDirs = projectsDir.listFiles { file -> file.isDirectory } ?: return emptyList()
        val projects = mutableListOf<Project>()
        for (dir in projectDirs) {
            try {
                val jsonFile = File(dir, "project.json")
                if (jsonFile.exists()) {
                    val project = gson.fromJson(jsonFile.readText(), Project::class.java)
                    projects.add(project)
                }
            } catch (e: Exception) {
                e.printStackTrace()
            }
        }
        return projects
    }

    fun getVolumes(projectId: String): List<Volume> {
        val volumesDir = File(workspaceDir, "projects/$projectId/volumes")
        val volumeDirs = volumesDir.listFiles { file -> file.isDirectory } ?: return emptyList()
        val volumes = mutableListOf<Volume>()
        for (dir in volumeDirs) {
            try {
                val jsonFile = File(dir, "volume.json")
                if (jsonFile.exists()) {
                    val volume = gson.fromJson(jsonFile.readText(), Volume::class.java)
                    volumes.add(volume)
                }
            } catch (e: Exception) {
                e.printStackTrace()
            }
        }
        return volumes
    }

    fun getChapters(projectId: String, volumeId: String): List<ChapterMeta> {
        val chaptersDir = File(workspaceDir, "projects/$projectId/volumes/$volumeId/chapters")
        val chapterDirs = chaptersDir.listFiles { file -> file.isDirectory } ?: return emptyList()
        val chapters = mutableListOf<ChapterMeta>()
        for (dir in chapterDirs) {
            try {
                val jsonFile = File(dir, "chapter.meta.json")
                if (jsonFile.exists()) {
                    val chapter = gson.fromJson(jsonFile.readText(), ChapterMeta::class.java)
                    chapters.add(chapter)
                }
            } catch (e: Exception) {
                e.printStackTrace()
            }
        }
        return chapters
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): String {
        val mdFile = File(workspaceDir, "projects/$projectId/volumes/$volumeId/chapters/$chapterId/chapter.md")
        return if (mdFile.exists()) {
            mdFile.readText()
        } else {
            ""
        }
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): Boolean {
        val chapterDir = File(workspaceDir, "projects/$projectId/volumes/$volumeId/chapters/$chapterId")
        if (!chapterDir.exists()) {
            chapterDir.mkdirs()
        }
        val mdFile = File(chapterDir, "chapter.md")
        val tmpFile = File(chapterDir, "chapter.md.tmp")

        try {
            tmpFile.writeText(content)
            val success = tmpFile.renameTo(mdFile)
            if (!success) return false

            val metaFile = File(chapterDir, "chapter.meta.json")
            if (metaFile.exists()) {
                val chapterMeta = gson.fromJson(metaFile.readText(), ChapterMeta::class.java)
                val updatedMeta = chapterMeta.copy(
                    updatedAt = getCurrentTimeStr(),
                    wordCount = content.length
                )
                val metaTmpFile = File(chapterDir, "chapter.meta.json.tmp")
                metaTmpFile.writeText(gson.toJson(updatedMeta))
                val metaSuccess = metaTmpFile.renameTo(metaFile)
                if (!metaSuccess) return false
            }
            return true

        } catch (e: Exception) {
            e.printStackTrace()
            return false
        }
    }

    fun createProject(title: String): Project {
        val projectId = "proj_" + UUID.randomUUID().toString().take(8)
        val projectDir = File(workspaceDir, "projects/$projectId")
        projectDir.mkdirs()

        val timeStr = getCurrentTimeStr()
        val project = Project(projectId, title, timeStr, timeStr)
        val jsonFile = File(projectDir, "project.json")
        jsonFile.writeText(gson.toJson(project))

        createVolume(projectId, "Volume 1")

        return project
    }

    fun createVolume(projectId: String, title: String): Volume {
        val volumeId = "vol_" + UUID.randomUUID().toString().take(8)
        val volumeDir = File(workspaceDir, "projects/$projectId/volumes/$volumeId")
        volumeDir.mkdirs()

        val timeStr = getCurrentTimeStr()
        val volume = Volume(volumeId, title, timeStr, timeStr)
        val jsonFile = File(volumeDir, "volume.json")
        jsonFile.writeText(gson.toJson(volume))

        return volume
    }

    fun createChapter(projectId: String, volumeId: String, title: String): ChapterMeta {
        val chapterId = "chap_" + UUID.randomUUID().toString().take(8)
        val chapterDir = File(workspaceDir, "projects/$projectId/volumes/$volumeId/chapters/$chapterId")
        chapterDir.mkdirs()

        val timeStr = getCurrentTimeStr()
        val chapterMeta = ChapterMeta(chapterId, title, timeStr, timeStr, 0, "")

        val jsonFile = File(chapterDir, "chapter.meta.json")
        jsonFile.writeText(gson.toJson(chapterMeta))

        val mdFile = File(chapterDir, "chapter.md")
        mdFile.writeText("")

        return chapterMeta
    }
}
