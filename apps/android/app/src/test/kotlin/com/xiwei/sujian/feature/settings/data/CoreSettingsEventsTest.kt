package com.xiwei.sujian.feature.settings.data
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #618 三：设置事件总线按消费用途拆成三条独立事件 — 本机保存、外部设置同步、
 * 外部主题目录同步不再共用一个模糊全局事件。
 *
 * 语义：
 * - notifyLocalEditorSettingsChanged：只发 editorSettingsChanged（WritingPane 刷新编辑器设置），
 *   不再触发 SettingsViewModel 重读 / ThemeController 重载；
 * - notifyExternalSettingsChanged：发 externalSettingsChanged + editorSettingsChanged；
 * - notifyExternalThemeCatalogChanged：只发 themeCatalogChanged。
 */
@OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
class CoreSettingsEventsTest {
    @Test
    fun localEditorSettingsChanged_onlyEmitsEditorFlow() =
        runTest {
            val externalReceived = mutableListOf<Unit>()
            val editorReceived = mutableListOf<Unit>()
            val themeReceived = mutableListOf<Unit>()
            val job1 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.externalSettingsChanged.collect { externalReceived.add(it) }
                }
            val job2 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.editorSettingsChanged.collect { editorReceived.add(it) }
                }
            val job3 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.themeCatalogChanged.collect { themeReceived.add(it) }
                }
            CoreSettingsEvents.notifyLocalEditorSettingsChanged()
            testScheduler.advanceUntilIdle()
            job1.cancel()
            job2.cancel()
            job3.cancel()
            // 本机保存只发编辑器事件，不再冒充外部设置变化、不再触发主题重载。
            assertEquals(0, externalReceived.size)
            assertEquals(1, editorReceived.size)
            assertEquals(0, themeReceived.size)
        }

    @Test
    fun externalSettingsChanged_emitsExternalAndEditor() =
        runTest {
            val externalReceived = mutableListOf<Unit>()
            val editorReceived = mutableListOf<Unit>()
            val themeReceived = mutableListOf<Unit>()
            val job1 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.externalSettingsChanged.collect { externalReceived.add(it) }
                }
            val job2 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.editorSettingsChanged.collect { editorReceived.add(it) }
                }
            val job3 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.themeCatalogChanged.collect { themeReceived.add(it) }
                }
            CoreSettingsEvents.notifyExternalSettingsChanged()
            testScheduler.advanceUntilIdle()
            job1.cancel()
            job2.cancel()
            job3.cancel()
            assertEquals(1, externalReceived.size)
            assertEquals(1, editorReceived.size)
            assertEquals(0, themeReceived.size)
        }

    @Test
    fun themeCatalogChanged_onlyEmitsThemeFlow() =
        runTest {
            val externalReceived = mutableListOf<Unit>()
            val editorReceived = mutableListOf<Unit>()
            val themeReceived = mutableListOf<Unit>()
            val job1 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.externalSettingsChanged.collect { externalReceived.add(it) }
                }
            val job2 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.editorSettingsChanged.collect { editorReceived.add(it) }
                }
            val job3 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.themeCatalogChanged.collect { themeReceived.add(it) }
                }
            CoreSettingsEvents.notifyExternalThemeCatalogChanged()
            testScheduler.advanceUntilIdle()
            job1.cancel()
            job2.cancel()
            job3.cancel()
            // 主题目录同步只发 themeCatalogChanged：不重读设置、不打扰编辑器。
            assertEquals(0, externalReceived.size)
            assertEquals(0, editorReceived.size)
            assertEquals(1, themeReceived.size)
        }

    @Test
    fun multipleCalls_emitMultiple() =
        runTest {
            val externalReceived = mutableListOf<Unit>()
            val job1 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.externalSettingsChanged.collect { externalReceived.add(it) }
                }
            CoreSettingsEvents.notifyExternalSettingsChanged()
            testScheduler.advanceUntilIdle()
            CoreSettingsEvents.notifyExternalSettingsChanged()
            testScheduler.advanceUntilIdle()
            job1.cancel()
            assertEquals(2, externalReceived.size)
        }

    @Test
    fun noSubscriber_doesNotCrash() =
        runTest {
            CoreSettingsEvents.notifyLocalEditorSettingsChanged()
            CoreSettingsEvents.notifyExternalSettingsChanged()
            CoreSettingsEvents.notifyExternalThemeCatalogChanged()
            testScheduler.advanceUntilIdle()
            // 无订阅者时 tryEmit 返回 false 但不抛异常
        }
}
