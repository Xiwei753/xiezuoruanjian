package com.xiwei.writerapp.model

import com.xiwei.writerapp.model.*
import com.xiwei.writerapp.data.*
import com.xiwei.writerapp.ui.*


import com.google.gson.annotations.SerializedName

data class WorkspaceManifest(
    val version: Int
)

data class LocalSettings(
    val themeMode: String? = "system",
    val locale: String? = null,
    val editorFontSize: Float = 16f,
    val editorLineSpacingMultiplier: Float = 1.5f,
    val autoSaveEnabled: Boolean = true,
    val autoSaveDelayMs: Long = 1500L
)

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
