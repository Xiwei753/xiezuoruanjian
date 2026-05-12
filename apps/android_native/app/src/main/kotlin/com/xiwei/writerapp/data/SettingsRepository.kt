package com.xiwei.writerapp.data

import android.content.Context
import com.google.gson.Gson
import com.xiwei.writerapp.model.LocalSettings
import java.io.File

class SettingsRepository(context: Context) {
    private val gson = Gson()
    private val workspaceDir = WorkspaceManager.getWorkspaceDir(context)
    private val settingsDir = File(workspaceDir, "app-meta/settings")
    private val settingsFile = File(settingsDir, "settings.local.json")

    init {
        if (!settingsDir.exists()) {
            settingsDir.mkdirs()
        }
    }

    fun getLocalSettings(): LocalSettings {
        if (!settingsFile.exists()) {
            return LocalSettings()
        }
        return try {
            val content = settingsFile.readText()
            gson.fromJson(content, LocalSettings::class.java) ?: LocalSettings()
        } catch (e: Exception) {
            e.printStackTrace()
            LocalSettings()
        }
    }

    fun saveLocalSettings(settings: LocalSettings): Boolean {
        return try {
            if (!settingsDir.exists()) {
                settingsDir.mkdirs()
            }
            val tmpFile = File(settingsDir, "settings.local.json.tmp")
            tmpFile.writeText(gson.toJson(settings))
            tmpFile.renameTo(settingsFile)
        } catch (e: Exception) {
            e.printStackTrace()
            false
        }
    }
}
