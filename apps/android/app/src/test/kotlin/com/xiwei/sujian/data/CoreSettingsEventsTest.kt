package com.xiwei.sujian.data

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
}
