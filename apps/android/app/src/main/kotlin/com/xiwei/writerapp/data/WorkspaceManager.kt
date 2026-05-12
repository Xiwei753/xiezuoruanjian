package com.xiwei.writerapp.data

import android.content.Context
import java.io.File
import java.io.FileOutputStream
import java.io.IOException

object WorkspaceManager {
    fun getWorkspaceDir(context: Context): File {
        return File(context.filesDir, "workspace")
    }

    fun initWorkspaceIfNeeded(context: Context) {
        val workspaceDir = getWorkspaceDir(context)
        if (!workspaceDir.exists() || workspaceDir.list()?.isEmpty() == true) {
            // Copy sample workspace from assets
            copyAssetFolder(context, "sample_workspace", workspaceDir)
        }
    }

    private fun copyAssetFolder(context: Context, assetPath: String, targetDir: File) {
        try {
            val assets = context.assets.list(assetPath)
            if (assets.isNullOrEmpty()) {
                val targetFile = targetDir
                if (targetFile.parentFile?.exists() != true) {
                    targetFile.parentFile?.mkdirs()
                }
                context.assets.open(assetPath).use { inputStream ->
                    FileOutputStream(targetFile).use { outputStream ->
                        inputStream.copyTo(outputStream)
                    }
                }
            } else {
                if (!targetDir.exists()) {
                    targetDir.mkdirs()
                }
                for (asset in assets) {
                    val subAssetPath = if (assetPath.isEmpty()) asset else "$assetPath/$asset"
                    val subTargetFile = File(targetDir, asset)
                    copyAssetFolder(context, subAssetPath, subTargetFile)
                }
            }
        } catch (e: IOException) {
            e.printStackTrace()
        }
    }
}
