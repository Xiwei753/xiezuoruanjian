package com.xiwei.sujian.support

import android.content.Context
import androidx.test.core.app.ActivityScenario
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.SettingsRepository
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.data.WriterAppServiceHolder
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.runtime.SujianAppDependencies
import com.xiwei.sujian.ui.MainActivity
import uniffi.writer_core.PlatformDto
import uniffi.writer_core.PlatformInitDto
import org.junit.Assert
import org.junit.rules.TestRule
import org.junit.runners.model.Statement
import org.junit.runner.Description
import java.io.File
import java.util.UUID

class TestSession private constructor(
    val testRootDir: File,
    val workspaceDir: File,
    val prefsSuffix: String,
    private var depsHolder: TestSujianAppDependencies,
    private val context: Context
) {
    val deps: TestSujianAppDependencies
        get() = depsHolder

    private var scenario: ActivityScenario<MainActivity>? = null

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

            val prefsSuffix = sessionId.take(8)

            val deps = TestSujianAppDependencies(
                appContext,
                testRootDir = testRootDir,
                workspaceDir = workspaceDir,
                prefsSuffix = prefsSuffix
            )

            return TestSession(
                testRootDir = testRootDir,
                workspaceDir = workspaceDir,
                prefsSuffix = prefsSuffix,
                depsHolder = deps,
                context = appContext
            )
        }
    }

    fun launchActivity() {
        scenario?.close()
        SujianAppDependencies.setTestProvider { _ -> depsHolder }
        scenario = ActivityScenario.launch(MainActivity::class.java)
    }

    fun closeActivity() {
        scenario?.close()
        scenario = null
    }

    fun <T> withActivity(block: (MainActivity) -> T): T {
        val sc = scenario ?: throw IllegalStateException("No active ActivityScenario. Call launchActivity() first.")
        var result: Any? = null
        sc.onActivity { result = block(it) }
        @Suppress("UNCHECKED_CAST")
        return result as T
    }

    fun restartRuntimeAndActivity() {
        depsHolder.releaseRuntime()
        depsHolder = TestSujianAppDependencies(
            context,
            testRootDir = testRootDir,
            workspaceDir = workspaceDir,
            prefsSuffix = prefsSuffix
        )
        SujianAppDependencies.setTestProvider { _ -> depsHolder }
    }

    fun release() {
        closeActivity()
        depsHolder.releaseRuntime()
        try {
            testRootDir.deleteRecursively()
        } catch (e: Exception) {
            throw AssertionError(
                "TestSession.release: Failed to delete test root directory ${testRootDir.absolutePath}: ${e.message}"
            )
        }
    }
}

class TestSujianAppDependencies(
    context: Context,
    testRootDir: File? = null,
    workspaceDir: File? = null,
    prefsSuffix: String = ""
) : SujianAppDependencies {
    private val appContext = context.applicationContext
    private val resolvedTestRoot = testRootDir ?: File(appContext.cacheDir, "test_workspace_${UUID.randomUUID()}")
    private val testWorkspaceDir = workspaceDir ?: File(resolvedTestRoot, "workspace")
    private val testAppDataDir = File(resolvedTestRoot, "app_data")
    private val testCacheDir = File(resolvedTestRoot, "cache")
    private val testLogDir = File(resolvedTestRoot, "logs")
    private val testNoBackupDir = File(resolvedTestRoot, "no_backup")

    init {
        testAppDataDir.mkdirs()
        testCacheDir.mkdirs()
        testLogDir.mkdirs()
        testNoBackupDir.mkdirs()
        if (!testWorkspaceDir.exists()) {
            testWorkspaceDir.mkdirs()
            File(testWorkspaceDir, "projects").mkdirs()
            File(testWorkspaceDir, "app-meta/settings").mkdirs()
            File(testWorkspaceDir, "app-meta/logs").mkdirs()
            File(testWorkspaceDir, "trash").mkdirs()
            File(testWorkspaceDir, "sqlite_cache").mkdirs()
            val manifest = File(testWorkspaceDir, "workspace_manifest.json")
            if (!manifest.exists()) {
                manifest.writeText("{\"version\": 1}")
            }
        }
    }

    private val testHolder: WriterAppServiceHolder = WriterAppServiceHolder(
        workspacePath = testWorkspaceDir.absolutePath,
        platformInit = PlatformInitDto(
            platform = PlatformDto.ANDROID,
            appDataDir = testAppDataDir.absolutePath,
            cacheDir = testCacheDir.absolutePath,
            logDir = testLogDir.absolutePath,
            noBackupDir = testNoBackupDir.absolutePath,
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
    override val settingsRepository: SettingsRepository = SettingsRepository(appContext, appServiceBridge, prefsSuffix)
    private val _coordinator = lazy { AnimatedTextEditorCoordinator(appContext, appServiceBridge) }
    override val coordinator: AnimatedTextEditorCoordinator get() = _coordinator.value

    fun releaseRuntime() {
        if (_coordinator.isInitialized()) {
            coordinator.releaseHost()
        }
    }

    override fun release() {
        releaseRuntime()
        try {
            resolvedTestRoot.deleteRecursively()
        } catch (e: Exception) {
            throw AssertionError(
                "TestSujianAppDependencies.release: Failed to delete test directory ${resolvedTestRoot.absolutePath}: ${e.message}"
            )
        }
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
                    val session = createSession(ctx)
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
