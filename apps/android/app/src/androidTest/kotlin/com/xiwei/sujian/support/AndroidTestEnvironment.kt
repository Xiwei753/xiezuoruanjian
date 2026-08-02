package com.xiwei.sujian.support

import android.content.Context
import androidx.test.core.app.ActivityScenario
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
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
import java.util.UUID
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import androidx.test.runner.lifecycle.ActivityLifecycleCallback
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage

class TestSession private constructor(
    val testRootDir: File,
    val workspaceDir: File,
    val prefsSuffix: String,
    private var depsHolder: TestSujianAppDependencies,
    private val context: Context,
    private val animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource,
    private val transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource,
    private val manualFrameClock: com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock.ManualFrameClock?
) {
    private val prefsFileNames = listOf(
        "sujian_diagnostics_$prefsSuffix",
        "sujian_device_$prefsSuffix",
        "writer_stats",
        "sujian_device",
        "sujian_experiments",
        "sujian_diagnostics"
    )
    private val dataStoreDirNames = listOf(
        "workbench_layout_prefs"
    )
    val deps: TestSujianAppDependencies
        get() = depsHolder

    companion object {
        fun create(
            context: Context,
            animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource = com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource(),
            transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource = com.xiwei.sujian.editor.v2.visual.TransactionIdSource(),
            manualFrameClock: com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock.ManualFrameClock? = null
        ): TestSession {
            val appContext = context.applicationContext
            val paths = TestWorkspaceFactory.createIsolatedWorkspace(appContext)

            val prefsSuffix = UUID.randomUUID().toString().take(8)

            val deps = TestSujianAppDependencies(
                appContext,
                testRootDir = paths.testRootDir,
                workspaceDir = paths.workspaceDir,
                prefsSuffix = prefsSuffix,
                animationTimeSource = animationTimeSource,
                transactionIdSource = transactionIdSource,
                manualFrameClock = manualFrameClock
            )

            return TestSession(
                testRootDir = paths.testRootDir,
                workspaceDir = paths.workspaceDir,
                prefsSuffix = prefsSuffix,
                depsHolder = deps,
                context = appContext,
                animationTimeSource = animationTimeSource,
                transactionIdSource = transactionIdSource,
                manualFrameClock = manualFrameClock
            )
        }
    }

    fun recreateDeps(): TestSujianAppDependencies {
        depsHolder = TestSujianAppDependencies(
            context,
            testRootDir = testRootDir,
            workspaceDir = workspaceDir,
            prefsSuffix = prefsSuffix,
            animationTimeSource = animationTimeSource,
            transactionIdSource = transactionIdSource,
            manualFrameClock = manualFrameClock
        )
        SujianAppDependencies.setTestProvider { _ -> depsHolder }
        return depsHolder
    }

    fun releaseSession() {
        var firstException: Throwable? = null
        try {
            depsHolder.releaseRuntime()
        } catch (t: Throwable) {
            firstException = t
        }
        val appContext = context.applicationContext
        for (name in prefsFileNames) {
            try {
                val prefs = appContext.getSharedPreferences(name, android.content.Context.MODE_PRIVATE)
                val cleared = prefs.edit().clear().commit()
                Assert.assertTrue("Failed to clear test SharedPreferences: $name", cleared)
                val deleted = appContext.deleteSharedPreferences(name)
                Assert.assertTrue("Failed to delete test SharedPreferences: $name", deleted)
            } catch (t: Throwable) {
                if (firstException != null) firstException.addSuppressed(t) else firstException = t
            }
        }
        for (dsName in dataStoreDirNames) {
            try {
                val dsDir = File(appContext.filesDir, "datastore/$dsName")
                if (dsDir.exists()) {
                    val deleted = dsDir.deleteRecursively()
                    Assert.assertTrue("Failed to delete test DataStore dir: $dsName (path=${dsDir.absolutePath})", deleted || !dsDir.exists())
                }
            } catch (t: Throwable) {
                if (firstException != null) firstException.addSuppressed(t) else firstException = t
            }
        }
        try {
            TestWorkspaceFactory.deleteWorkspace(TestWorkspaceFactory.TestWorkspacePaths(
                testRootDir = testRootDir,
                workspaceDir = workspaceDir,
                appDataDir = File(testRootDir, "app_data"),
                cacheDir = File(testRootDir, "cache"),
                logDir = File(testRootDir, "logs"),
                noBackupDir = File(testRootDir, "no_backup"),
            ))
        } catch (t: Throwable) {
            if (firstException != null) firstException.addSuppressed(t) else firstException = t
        }
        if (firstException != null) throw firstException
    }
}

class TestSujianAppDependencies(
    context: Context,
    testRootDir: File? = null,
    workspaceDir: File? = null,
    prefsSuffix: String = "",
    private val animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource = com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource(),
    private val transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource = com.xiwei.sujian.editor.v2.visual.TransactionIdSource(),
    val manualFrameClock: com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock.ManualFrameClock? = null
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
            throw AssertionError(
                "TestSujianAppDependencies: workspace dir does not exist — " +
                    "TestWorkspaceFactory.createIsolatedWorkspace() must be called first: " +
                    testWorkspaceDir.absolutePath
            )
        }
        val manifest = File(testWorkspaceDir, "workspace_manifest.json")
        if (!manifest.exists()) {
            throw AssertionError(
                "TestSujianAppDependencies: workspace_manifest.json does not exist — " +
                    "TestWorkspaceFactory.initializeWorkspaceViaCore() must have failed silently: " +
                    manifest.absolutePath
            )
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
    private val _frameClock = manualFrameClock?.let { com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock(it) }
    private val _coordinator = lazy { AnimatedTextEditorCoordinator(appContext, appServiceBridge, animationTimeSource, transactionIdSource, _frameClock) }
    override val coordinator: AnimatedTextEditorCoordinator get() = _coordinator.value
    private var runtimeReleased = false

    fun releaseRuntime() {
        if (runtimeReleased) return
        runtimeReleased = true
        if (_coordinator.isInitialized()) {
            coordinator.releaseHost()
        }
        testHolder.close()
    }

    override fun release() {
        releaseRuntime()
    }
}

class RestartableMainActivityRule(
    private val sessionProvider: () -> TestSession
) : TestRule {
    private var scenario: ActivityScenario<MainActivity>? = null
    private var composeTestRule: androidx.compose.ui.test.junit4.ComposeTestRule? = null

    fun setComposeTestRule(rule: androidx.compose.ui.test.junit4.ComposeTestRule) {
        composeTestRule = rule
    }

    private fun provideActivity(): MainActivity {
        val sc = scenario ?: throw IllegalStateException("No active ActivityScenario. Call launchActivity() first.")
        var activity: MainActivity? = null
        sc.onActivity { activity = it }
        return activity ?: throw IllegalStateException("ActivityScenario.onActivity did not provide activity.")
    }

    val composeActivityProvider: (RestartableMainActivityRule) -> MainActivity
        get() = { _ -> provideActivity() }

    fun launchActivity() {
        scenario?.close()
        val session = sessionProvider()
        SujianAppDependencies.setTestProvider { _ -> session.deps }
        scenario = ActivityScenario.launch(MainActivity::class.java)
        scenario!!.onActivity { }
    }

    fun restartRuntimeAndActivity() {
        val sc = scenario
        if (sc != null) {
            val destroyLatch = CountDownLatch(1)
            val lifecycleCallback = ActivityLifecycleCallback { activity, stage ->
                if (stage == Stage.DESTROYED && activity is MainActivity) {
                    destroyLatch.countDown()
                }
            }
            val lifecycleMonitor = ActivityLifecycleMonitorRegistry.getInstance()
            lifecycleMonitor.addLifecycleCallback(lifecycleCallback)

            sc.close()
            scenario = null

            val destroyed = destroyLatch.await(10, TimeUnit.SECONDS)
            lifecycleMonitor.removeLifecycleCallback(lifecycleCallback)

            Assert.assertTrue(
                "Old Activity was not destroyed within 10 seconds after scenario.close()",
                destroyed
            )
        }

        val session = sessionProvider()
        session.deps.releaseRuntime()
        session.recreateDeps()

        SujianAppDependencies.setTestProvider { _ -> session.deps }
        scenario = ActivityScenario.launch(MainActivity::class.java)

        val resumeLatch = CountDownLatch(1)
        val maxWaitMs = 10_000L
        val resumeStartMs = System.currentTimeMillis()
        while (!resumeLatch.await(0, TimeUnit.MILLISECONDS) && (System.currentTimeMillis() - resumeStartMs) < maxWaitMs) {
            scenario!!.onActivity { activity ->
                if (activity.lifecycle.currentState.isAtLeast(androidx.lifecycle.Lifecycle.State.RESUMED)) {
                    resumeLatch.countDown()
                }
            }
        }

        Assert.assertTrue(
            "New Activity did not reach RESUMED state within ${maxWaitMs}ms after cold restart",
            resumeLatch.await(0, TimeUnit.MILLISECONDS)
        )

        composeTestRule?.waitForIdle()
    }

    fun <T> onActivity(action: (MainActivity) -> T): T {
        val sc = scenario ?: throw IllegalStateException("No active ActivityScenario. Call launchActivity() first.")
        var result: T? = null
        sc.onActivity { activity ->
            result = action(activity)
        }
        return result ?: throw IllegalStateException("onActivity did not produce a result.")
    }

    fun simulateBackgroundRecovery() {
        val sc = scenario ?: throw IllegalStateException("No active ActivityScenario. Call launchActivity() first.")
        sc.moveToState(androidx.lifecycle.Lifecycle.State.STARTED)
        sc.moveToState(androidx.lifecycle.Lifecycle.State.RESUMED)
        composeTestRule?.waitForIdle()
    }

    fun isActivityLaunched(): Boolean {
        return scenario != null
    }

    fun closeActivity() {
        scenario?.close()
        scenario = null
    }

    override fun apply(base: Statement, description: Description): Statement {
        return object : Statement() {
            override fun evaluate() {
                try {
                    launchActivity()
                    base.evaluate()
                } finally {
                    closeActivity()
                }
            }
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

    fun createSession(
        context: Context,
        animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource = com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource(),
        transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource = com.xiwei.sujian.editor.v2.visual.TransactionIdSource(),
        manualFrameClock: com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock.ManualFrameClock? = null
    ): TestSession {
        val session = TestSession.create(context, animationTimeSource, transactionIdSource, manualFrameClock)
        currentSession = session
        return session
    }

    fun releaseSession() {
        try {
            currentSession?.releaseSession()
        } finally {
            currentSession = null
        }
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

    class TestDependenciesRule(
        private val animationTimeSource: com.xiwei.sujian.editor.v2.visual.AnimationTimeSource = com.xiwei.sujian.editor.v2.visual.ChoreographerAnimationTimeSource(),
        private val transactionIdSource: com.xiwei.sujian.editor.v2.visual.TransactionIdSource = com.xiwei.sujian.editor.v2.visual.TransactionIdSource(),
        val manualFrameClock: com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock.ManualFrameClock? = null
    ) : TestRule {
        override fun apply(base: Statement, description: Description): Statement {
            return object : Statement() {
                override fun evaluate() {
                    val instrumentation = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation()
                    val ctx = instrumentation.targetContext
                    val session = createSession(ctx, animationTimeSource, transactionIdSource, manualFrameClock)
                    SujianAppDependencies.setTestProvider { _ -> session.deps }
                    var testFailed: Throwable? = null
                    try {
                        base.evaluate()
                    } catch (t: Throwable) {
                        testFailed = t
                    } finally {
                        SujianAppDependencies.setTestProvider(null)
                        try {
                            releaseSession()
                        } catch (cleanup: Throwable) {
                            if (testFailed != null) {
                                testFailed.addSuppressed(cleanup)
                            } else {
                                testFailed = cleanup
                            }
                        }
                    }
                    if (testFailed != null) throw testFailed
                }
            }
        }
    }
}
