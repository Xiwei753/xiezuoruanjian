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

class TestSujianAppDependencies(
    context: Context
) : SujianAppDependencies {
    private val appContext = context.applicationContext
    private val testWorkspaceDir = File(appContext.cacheDir, "test_workspace_${UUID.randomUUID()}")
    private val testHolder: WriterAppServiceHolder = WriterAppServiceHolder(
        workspacePath = testWorkspaceDir.absolutePath,
        platformInit = PlatformInitDto(
            platform = PlatformDto.ANDROID,
            appDataDir = appContext.filesDir.absolutePath,
            cacheDir = appContext.cacheDir.absolutePath,
            logDir = "${appContext.cacheDir.absolutePath}/log",
            noBackupDir = appContext.noBackupFilesDir.absolutePath,
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
    fun create(context: Context): TestSujianAppDependencies {
        return TestSujianAppDependencies(context)
    }

    fun installTestDependencies() {
        SujianAppDependencies.setTestProvider { ctx ->
            TestSujianAppDependencies(ctx)
        }
    }

    fun clearTestDependencies() {
        SujianAppDependencies.setTestProvider(null)
    }

    data class TestProjectData(
        val projectId: String,
        val projectTitle: String,
        val volumeId: String,
        val volumeTitle: String,
    )

    fun ensureTestProjectAndVolume(context: Context, deps: TestSujianAppDependencies? = null): TestProjectData {
        val repo = deps?.workspaceRepository ?: WorkspaceRepository(context)
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
                    val ctx = androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().targetContext
                    val testDeps = TestSujianAppDependencies(ctx)
                    SujianAppDependencies.setTestProvider { _ -> testDeps }
                    try {
                        base.evaluate()
                    } finally {
                        SujianAppDependencies.setTestProvider(null)
                    }
                }
            }
        }
    }
}
