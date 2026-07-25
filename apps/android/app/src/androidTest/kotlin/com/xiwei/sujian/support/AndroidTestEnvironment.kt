package com.xiwei.sujian.support

import android.content.Context
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WriterAppServiceHolder
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.runtime.SujianAppDependencies
import uniffi.writer_core.PlatformDto
import uniffi.writer_core.PlatformInitDto
import org.junit.rules.TestRule
import org.junit.runners.model.Statement
import org.junit.runner.Description
import java.io.File
import java.util.UUID

class TestSession private constructor(
    val testRootDir: File,
    val workspaceDir: File,
    val deps: TestSujianAppDependencies
) {
    companion object {
        fun create(context: Context): TestSession {
            val appContext = context.applicationContext
            val sessionId = UUID.randomUUID().toString()
            val testRootDir = File(appContext.cacheDir, "test_session_$sessionId")
            testRootDir.mkdirs()

            val workspaceDir = File(testRootDir, "workspace")
            workspaceDir.mkdirs()
            File(workspaceDir, "projects").mkdirs()
            File(workspaceDir, "app-meta/settings").mkdirs()
            File(workspaceDir, "app-meta/logs").mkdirs()
            File(workspaceDir, "trash").mkdirs()
            File(workspaceDir, "sqlite_cache").mkdirs()
            val manifest = File(workspaceDir, "workspace_manifest.json")
            if (!manifest.exists()) {
                manifest.writeText("{\"version\": 1}")
            }

            val deps = TestSujianAppDependencies(
                appContext,
                workspaceDir.absolutePath
            )

            return TestSession(
                testRootDir = testRootDir,
                workspaceDir = workspaceDir,
                deps = deps
            )
        }
    }

    fun release() {
        deps.release()
        try {
            testRootDir.deleteRecursively()
        } catch (_: Exception) { }
    }
}

class TestSujianAppDependencies(
    context: Context,
    workspacePath: String? = null,
    appDataPath: String? = null,
    cachePath: String? = null,
    logPath: String? = null,
    noBackupPath: String? = null
) : SujianAppDependencies {
    private val appContext = context.applicationContext
    private val testWorkspaceDir = File(
        workspacePath ?: "${appContext.cacheDir}/test_workspace_${UUID.randomUUID()}"
    )
    private val testAppDataDir = appDataPath ?: appContext.filesDir.absolutePath
    private val testCacheDir = cachePath ?: appContext.cacheDir.absolutePath
    private val testLogDir = logPath ?: "${appContext.cacheDir.absolutePath}/log"
    private val testNoBackupDir = noBackupPath ?: appContext.noBackupFilesDir.absolutePath

    private val testHolder: WriterAppServiceHolder = WriterAppServiceHolder(
        workspacePath = testWorkspaceDir.absolutePath,
        platformInit = PlatformInitDto(
            platform = PlatformDto.ANDROID,
            appDataDir = testAppDataDir,
            cacheDir = testCacheDir,
            logDir = testLogDir,
            noBackupDir = testNoBackupDir,
            deviceId = "test-${UUID.randomUUID()}",
            appVersion = "test",
            locale = java.util.Locale.getDefault().toLanguageTag(),
            timezone = java.util.TimeZone.getDefault().id,
            isConnected = true,
            isMetered = false,
            proxyHost = null,
            proxyPort = null,
        ),
    )
    override val appServiceBridge: AppServiceBridge = AppServiceBridge(testHolder)
    override val workspaceRepository: WorkspaceRepository = WorkspaceRepository(appContext, appServiceBridge)
    override val settingsRepository: SettingsRepository = SettingsRepository(appContext, appServiceBridge)
    override val coordinator: AnimatedTextEditorCoordinator = AnimatedTextEditorCoordinator(appContext, appServiceBridge)

    private val settingsSnapshot = settingsRepository.getLocalSettings()

    override fun release() {
        coordinator.releaseHost()
        try {
            settingsRepository.saveLocalSettings(settingsSnapshot)
        } catch (_: Exception) { }
        try {
            testWorkspaceDir.deleteRecursively()
        } catch (_: Exception) { }
    }
}

object AndroidTestEnvironment {
    @Volatile
    private var currentSession: TestSession? = null

    fun requireCurrentSession(): TestSession {
        return currentSession
            ?: throw IllegalStateException("No TestSession active. Ensure TestDependenciesRule is applied.")
    }

    fun createSession(context: Context): TestSession {
        val session = TestSession.create(context)
        currentSession = session
        return session
    }

    fun releaseSession() {
        currentSession?.release()
        currentSession = null
    }

    data class TestProjectData(
        val projectId: String,
        val projectTitle: String,
        val volumeId: String,
        val volumeTitle: String,
    )

    fun ensureTestProjectAndVolume(context: Context, session: TestSession? = null): TestProjectData {
        val s = session ?: requireCurrentSession()
        val repo = s.deps.workspaceRepository
        val projects = repo.getProjects()
        val existing = projects.firstOrNull { it.title == "自动化测试作品" }
        if (existing != null) {
            val volumes = repo.getVolumes(existing.id)
            val volume = volumes.firstOrNull()
            if (volume != null) {
                return TestProjectData(existing.id, existing.title, volume.id, volume.title)
            }
        }
        val project = repo.createProject("自动化测试作品")
        val volume = repo.createVolume(project.id, "测试卷")
        return TestProjectData(project.id, project.title, volume.id, volume.title)
    }

    class TestDependenciesRule : TestRule {
        override fun apply(base: Statement, description: Description): Statement {
            return object : Statement() {
                override fun evaluate() {
                    val instrumentation = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation()
                    val ctx = instrumentation.targetContext
                    lateinit var session: TestSession
                    instrumentation.runOnMainSync {
                        session = createSession(ctx)
                    }
                    SujianAppDependencies.setTestProvider { _ -> session.deps }
                    try {
                        base.evaluate()
                    } finally {
                        SujianAppDependencies.setTestProvider(null)
                        releaseSession()
                    }
                }
            }
        }
    }
}
