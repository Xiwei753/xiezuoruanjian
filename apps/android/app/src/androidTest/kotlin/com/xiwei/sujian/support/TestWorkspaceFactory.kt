package com.xiwei.sujian.support

import android.content.Context
import com.xiwei.sujian.data.WriterAppServiceHolder
import java.io.File
import java.util.UUID

object TestWorkspaceFactory {
    data class TestWorkspacePaths(
        val testRootDir: File,
        val workspaceDir: File,
        val appDataDir: File,
        val cacheDir: File,
        val logDir: File,
        val noBackupDir: File,
    )

    fun createIsolatedWorkspace(appContext: Context): TestWorkspacePaths {
        val sessionId = UUID.randomUUID().toString()
        val testRootDir = File(appContext.cacheDir, "test_session_$sessionId")
        testRootDir.mkdirs()

        val workspaceDir = File(testRootDir, "workspace")
        val appDataDir = File(testRootDir, "app_data")
        val cacheDir = File(testRootDir, "cache")
        val logDir = File(testRootDir, "logs")
        val noBackupDir = File(testRootDir, "no_backup")

        appDataDir.mkdirs()
        cacheDir.mkdirs()
        logDir.mkdirs()
        noBackupDir.mkdirs()

        initializeWorkspaceViaCore(workspaceDir, appDataDir, cacheDir, logDir, noBackupDir)

        return TestWorkspacePaths(
            testRootDir = testRootDir,
            workspaceDir = workspaceDir,
            appDataDir = appDataDir,
            cacheDir = cacheDir,
            logDir = logDir,
            noBackupDir = noBackupDir,
        )
    }

    private fun initializeWorkspaceViaCore(
        workspaceDir: File,
        appDataDir: File,
        cacheDir: File,
        logDir: File,
        noBackupDir: File,
    ) {
        workspaceDir.mkdirs()
        // 新 Core API：openAppService 自动完成初始化，不再需要显式工作区创建。
        val initHolder =
            WriterAppServiceHolder(
                appDataRoot = appDataDir.absolutePath,
                projectsRoot = workspaceDir.absolutePath,
            )
        try {
            // 访问 service 触发 lazy 初始化，确保目录结构就绪。
            initHolder.service
        } finally {
            initHolder.close()
        }
    }

    fun deleteWorkspace(paths: TestWorkspacePaths) {
        val deleted = paths.testRootDir.deleteRecursively()
        if (!deleted && paths.testRootDir.exists()) {
            throw AssertionError(
                "TestWorkspaceFactory: Failed to delete test root directory " +
                    paths.testRootDir.absolutePath,
            )
        }
    }
}
