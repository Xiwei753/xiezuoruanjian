package com.xiwei.sujian.support

import android.content.Context
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import java.io.File
import java.util.UUID

object TestProjectEnvFactory {
    data class TestProjectEnvPaths(
        val testRootDir: File,
        val projectsDir: File,
        val appDataDir: File,
        val cacheDir: File,
        val logDir: File,
        val noBackupDir: File,
    )

    fun createIsolatedProjectEnv(appContext: Context): TestProjectEnvPaths {
        val sessionId = UUID.randomUUID().toString()
        val testRootDir = File(appContext.cacheDir, "test_session_$sessionId")
        testRootDir.mkdirs()

        val projectsDir = File(testRootDir, "projects")
        val appDataDir = File(testRootDir, "app_data")
        val cacheDir = File(testRootDir, "cache")
        val logDir = File(testRootDir, "logs")
        val noBackupDir = File(testRootDir, "no_backup")

        appDataDir.mkdirs()
        cacheDir.mkdirs()
        logDir.mkdirs()
        noBackupDir.mkdirs()

        initializeProjectEnvViaCore(projectsDir, appDataDir, cacheDir, logDir, noBackupDir)

        return TestProjectEnvPaths(
            testRootDir = testRootDir,
            projectsDir = projectsDir,
            appDataDir = appDataDir,
            cacheDir = cacheDir,
            logDir = logDir,
            noBackupDir = noBackupDir,
        )
    }

    private fun initializeProjectEnvViaCore(
        projectsDir: File,
        appDataDir: File,
        cacheDir: File,
        logDir: File,
        noBackupDir: File,
    ) {
        projectsDir.mkdirs()
        // 新 Core API：openAppService 自动完成初始化，不再需要显式 project env 创建。
        val initHolder =
            WriterAppServiceHolder(
                appDataRoot = appDataDir.absolutePath,
                projectsRoot = projectsDir.absolutePath,
            )
        try {
            // 访问 service 触发 lazy 初始化，确保目录结构就绪。
            initHolder.service
        } finally {
            initHolder.close()
        }
    }

    fun deleteProjectEnv(paths: TestProjectEnvPaths) {
        val deleted = paths.testRootDir.deleteRecursively()
        if (!deleted && paths.testRootDir.exists()) {
            throw AssertionError(
                "TestProjectEnvFactory: Failed to delete test root directory " +
                    paths.testRootDir.absolutePath,
            )
        }
    }
}
