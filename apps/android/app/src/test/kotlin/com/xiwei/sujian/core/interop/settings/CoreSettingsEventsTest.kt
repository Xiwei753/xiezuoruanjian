package com.xiwei.sujian.core.interop.settings
import com.xiwei.sujian.core.interop.common.ChangedEntity
import com.xiwei.sujian.core.interop.common.ResultEnvelope
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

@OptIn(kotlinx.coroutines.ExperimentalCoroutinesApi::class)
class CoreSettingsEventsTest {
    private val settingsEnvelope: ResultEnvelope<Unit> =
        ResultEnvelope(
            success = true,
            changedEntities = listOf(ChangedEntity("SettingsSaved", "")),
        )

    private val otherEnvelope: ResultEnvelope<Unit> =
        ResultEnvelope(
            success = true,
            changedEntities = listOf(ChangedEntity("OtherEntity", "")),
        )

    @Test
    fun settingsChanged_emitsOnRecord() =
        runTest {
            val received = mutableListOf<Unit>()
            val job =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.settingsChanged.collect { received.add(it) }
                }
            CoreSettingsEvents.record(settingsEnvelope)
            testScheduler.advanceUntilIdle()
            job.cancel()
            assertEquals(1, received.size)
        }

    @Test
    fun editorSettingsChanged_emitsOnRecord() =
        runTest {
            val received = mutableListOf<Unit>()
            val job =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.editorSettingsChanged.collect { received.add(it) }
                }
            CoreSettingsEvents.record(settingsEnvelope)
            testScheduler.advanceUntilIdle()
            job.cancel()
            assertEquals(1, received.size)
        }

    @Test
    fun markEditorChanged_emitsBothFlows() =
        runTest {
            val settingsReceived = mutableListOf<Unit>()
            val editorReceived = mutableListOf<Unit>()
            val job1 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.settingsChanged.collect { settingsReceived.add(it) }
                }
            val job2 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.editorSettingsChanged.collect { editorReceived.add(it) }
                }
            CoreSettingsEvents.markEditorChanged()
            testScheduler.advanceUntilIdle()
            job1.cancel()
            job2.cancel()
            assertEquals(1, settingsReceived.size)
            assertEquals(1, editorReceived.size)
        }

    @Test
    fun record_nonSettingsEntity_doesNotEmit() =
        runTest {
            val received = mutableListOf<Unit>()
            val job =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.settingsChanged.collect { received.add(it) }
                }
            CoreSettingsEvents.record(otherEnvelope)
            testScheduler.advanceUntilIdle()
            job.cancel()
            assertEquals(0, received.size)
        }

    /**
     * #600 评论 #7: notifySyncableSettingsChangedExternally / notifyPaletteCatalogChangedExternally
     * 委托 markEditorChanged — 验证连续两次调用都能触发事件.
     */
    @Test
    fun markEditorChanged_multipleCalls_emitMultiple() =
        runTest {
            val settingsReceived = mutableListOf<Unit>()
            val editorReceived = mutableListOf<Unit>()
            val job1 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.settingsChanged.collect { settingsReceived.add(it) }
                }
            val job2 =
                launch(UnconfinedTestDispatcher(testScheduler)) {
                    CoreSettingsEvents.editorSettingsChanged.collect { editorReceived.add(it) }
                }
            CoreSettingsEvents.markEditorChanged()
            testScheduler.advanceUntilIdle()
            CoreSettingsEvents.markEditorChanged()
            testScheduler.advanceUntilIdle()
            job1.cancel()
            job2.cancel()
            assertEquals(2, settingsReceived.size)
            assertEquals(2, editorReceived.size)
        }

    /**
     * #600 评论 #7 反面: 无订阅者时 markEditorChanged 不崩溃 — tryEmit 丢弃事件.
     */
    @Test
    fun markEditorChanged_noSubscriber_doesNotCrash() =
        runTest {
            CoreSettingsEvents.markEditorChanged()
            testScheduler.advanceUntilIdle()
            // 无订阅者, tryEmit 返回 false 但不抛异常
        }
}
