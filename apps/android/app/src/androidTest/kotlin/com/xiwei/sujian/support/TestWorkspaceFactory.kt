package com.xiwei.sujian.support

import android.content.Context
import com.xiwei.sujian.data.BridgeResult
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
        val initHolder = WriterAppServiceHolder(
            workspacePath = workspaceDir.absolutePath,
        )
        try {
            val result = initHolder.wrapResult { initHolder.service.createWorkspaceIfNeeded() }
            when (result) {
                is BridgeResult.Success -> {}
                is BridgeResult.Error -> throw AssertionError(
                    "TestWorkspaceFactory: createWorkspaceIfNeeded failed: " +
                        "errorCode=${result.code}, message=${result.message}, " +
                        "rawError=${result.envelope.rawError}"
                )
                BridgeResult.NotLoaded -> throw AssertionError(
                    "TestWorkspaceFactory: native library not loaded during workspace initialization"
                )
            }
        } finally {
            initHolder.close()
        }
    }

    fun deleteWorkspace(paths: TestWorkspacePaths) {
        try {
            paths.testRootDir.deleteRecursively()
        } catch (e: Exception) {
            throw AssertionError(
                "TestWorkspaceFactory: Failed to delete test root directory " +
                    "${paths.testRootDir.absolutePath}: ${e.message}"
            )
        }
    }
}
