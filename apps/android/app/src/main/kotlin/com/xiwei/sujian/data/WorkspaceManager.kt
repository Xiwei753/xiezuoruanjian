package com.xiwei.sujian.data

import android.content.Context
import android.util.Log
import java.io.File
import java.io.FileOutputStream
import java.io.IOException

private const val TAG = "WorkspaceManager"

/**
 * WorkspaceManager — 工作区目录管理
 *
 * 负责工作区目录的获取和初始化（从 assets 复制示例工作区）。
 *
 * ## 架构定位
 * - 全局单例，管理工作区目录路径
 * - 首次启动时从 assets 复制示例工作区
 *
 * ## 职责边界
 * - **做**：获取工作区目录、初始化示例工作区
 * - **不做**：业务逻辑（由 Rust Core 负责）
 *
 * ## 使用场景
 * - data 层 legacy adapter 获取工作区路径
 * - 首次启动应用时初始化工作区
 */
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
            Log.w(TAG, "Failed to copy workspace asset", e)
        }
    }
}
