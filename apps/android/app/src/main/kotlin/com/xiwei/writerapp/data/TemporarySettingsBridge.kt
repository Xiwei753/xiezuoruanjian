
package com.xiwei.writerapp.data

import android.content.Context
import com.google.gson.Gson
import com.xiwei.writerapp.model.LocalSettings
import java.io.File

/**
 * Temporary Bridge.
 * This class MUST be replaced with JNI calls to core/writer_core/src/facade.rs.
 * It is currently only retained as a fallback mechanism if the JNI library fails to load.
 */
@Deprecated("Temporary Bridge. Only used as a fallback if Native JNI fails. Do not use for new features.")
class TemporarySettingsBridge(context: Context) {
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
