package com.xiwei.sujian.feature.settings.data

import kotlinx.coroutines.launch
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #618 三：设置事件总线按消费用途拆成独立事件 — 本机保存、外部设置同步不共用
 * 一个模糊全局事件。
 *
 * Issue #1005：设备配色不再跨端同步，主题目录同步事件已删除。
 */
@OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
class CoreSettingsEventsTest {
    @Test
    fun localEditorSettingsChanged_onlyEmitsEditorFlow() =
        runTest {
            val externalReceived = mutableListOf<Unit>()
            val editorReceived = mutableListOf<Unit>()
            val job1 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.externalSettingsChanged.collect { externalReceived.add(it) }
                }
            val job2 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.editorSettingsChanged.collect { editorReceived.add(it) }
                }
            CoreSettingsEvents.notifyLocalEditorSettingsChanged()
            testScheduler.advanceUntilIdle()
            job1.cancel()
            job2.cancel()
            // 本机保存只发编辑器事件，不再冒充外部设置变化。
            assertEquals(0, externalReceived.size)
            assertEquals(1, editorReceived.size)
        }

    @Test
    fun externalSettingsChanged_emitsExternalAndEditor() =
        runTest {
            val externalReceived = mutableListOf<Unit>()
            val editorReceived = mutableListOf<Unit>()
            val job1 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.externalSettingsChanged.collect { externalReceived.add(it) }
                }
            val job2 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.editorSettingsChanged.collect { editorReceived.add(it) }
                }
            CoreSettingsEvents.notifyExternalSettingsChanged()
            testScheduler.advanceUntilIdle()
            job1.cancel()
            job2.cancel()
            assertEquals(1, externalReceived.size)
            assertEquals(1, editorReceived.size)
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
            testScheduler.advanceUntilIdle()
            // 无订阅者时 tryEmit 返回 false 但不抛异常
        }
}
