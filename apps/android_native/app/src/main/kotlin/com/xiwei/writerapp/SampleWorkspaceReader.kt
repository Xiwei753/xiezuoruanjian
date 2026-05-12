package com.xiwei.writerapp

import android.content.Context
import com.google.gson.Gson
import com.google.gson.annotations.SerializedName
import java.io.InputStreamReader

data class Project(
    val id: String,
    val title: String,
    @SerializedName("created_at") val createdAt: String,
    @SerializedName("updated_at") val updatedAt: String
)

data class Volume(
    val id: String,
    val title: String,
    @SerializedName("created_at") val createdAt: String,
    @SerializedName("updated_at") val updatedAt: String
)

data class ChapterMeta(
    val id: String,
    val title: String,
    @SerializedName("created_at") val createdAt: String,
    @SerializedName("updated_at") val updatedAt: String,
    @SerializedName("word_count") val wordCount: Int,
    val hash: String
)

class SampleWorkspaceReader(private val context: Context) {
    private val gson = Gson()
    private val basePath = "sample_workspace"

    fun getProjects(): List<Project> {
        val projectsPath = "$basePath/projects"
        val projectDirs = context.assets.list(projectsPath) ?: return emptyList()
        val projects = mutableListOf<Project>()
        for (dir in projectDirs) {
            try {
                val jsonPath = "$projectsPath/$dir/project.json"
                context.assets.open(jsonPath).use { inputStream ->
                    val project = gson.fromJson(InputStreamReader(inputStream), Project::class.java)
                    projects.add(project)
                }
            } catch (e: Exception) {
                e.printStackTrace()
            }
        }
        return projects
    }

    fun getVolumes(projectId: String): List<Volume> {
        val volumesPath = "$basePath/projects/$projectId/volumes"
        val volumeDirs = context.assets.list(volumesPath) ?: return emptyList()
        val volumes = mutableListOf<Volume>()
        for (dir in volumeDirs) {
            try {
                val jsonPath = "$volumesPath/$dir/volume.json"
                context.assets.open(jsonPath).use { inputStream ->
                    val volume = gson.fromJson(InputStreamReader(inputStream), Volume::class.java)
                    volumes.add(volume)
                }
            } catch (e: Exception) {
                e.printStackTrace()
            }
        }
        return volumes
    }

    fun getChapters(projectId: String, volumeId: String): List<ChapterMeta> {
        val chaptersPath = "$basePath/projects/$projectId/volumes/$volumeId/chapters"
        val chapterDirs = context.assets.list(chaptersPath) ?: return emptyList()
        val chapters = mutableListOf<ChapterMeta>()
        for (dir in chapterDirs) {
            try {
                val jsonPath = "$chaptersPath/$dir/chapter.meta.json"
                context.assets.open(jsonPath).use { inputStream ->
                    val chapter = gson.fromJson(InputStreamReader(inputStream), ChapterMeta::class.java)
                    chapters.add(chapter)
                }
            } catch (e: Exception) {
                e.printStackTrace()
            }
        }
        return chapters
    }

    fun getChapterContent(projectId: String, volumeId: String, chapterId: String): String {
        val mdPath = "$basePath/projects/$projectId/volumes/$volumeId/chapters/$chapterId/chapter.md"
        return try {
            context.assets.open(mdPath).bufferedReader().use { it.readText() }
        } catch (e: Exception) {
            e.printStackTrace()
            "Error loading chapter content."
        }
    }
}
