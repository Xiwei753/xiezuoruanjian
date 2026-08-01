package com.xiwei.sujian.editor

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextRange
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.WriterAppServiceHolder
import com.xiwei.sujian.editor.v2.compose.AnimatedTextField
import com.xiwei.sujian.editor.v2.compose.AnimatedTextArea
import com.xiwei.sujian.editor.v2.compose.AnimatedInlineText
import com.xiwei.sujian.editor.v2.compose.LocalAnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.coordinator.WindowDisplayFrameClock
import com.xiwei.sujian.editor.v2.visual.ManualAnimationTimeSource
import com.xiwei.sujian.editor.v2.visual.TransactionIdSource
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.writer_core.PlatformDto
import uniffi.writer_core.PlatformInitDto
import java.io.File

@RunWith(AndroidJUnit4::class)
class AnimatedTextSemanticsInstrumentedTest {

    @get:Rule
    val composeTestRule = createComposeRule()

    private lateinit var coordinator: AnimatedTextEditorCoordinator
    private lateinit var serviceHolder: WriterAppServiceHolder
    private lateinit var testDir: File

    @Before
    fun setUp() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        testDir = File(context.cacheDir, "semantics_test_${System.nanoTime()}")
        testDir.mkdirs()
        val workspaceDir = File(testDir, "workspace")
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

        serviceHolder = WriterAppServiceHolder(
            workspacePath = workspaceDir.absolutePath,
            platformInit = PlatformInitDto(
                platform = PlatformDto.ANDROID,
                appDataDir = testDir.absolutePath,
                cacheDir = testDir.absolutePath,
                logDir = testDir.absolutePath,
                noBackupDir = testDir.absolutePath,
                deviceId = "test",
                appVersion = "test",
                locale = "zh-CN",
                timezone = "Asia/Shanghai",
                isConnected = true,
                isMetered = false,
                proxyHost = null,
                proxyPort = null,
            ),
        )
        val bridge = AppServiceBridge(serviceHolder)
        val manualFrameClock = WindowDisplayFrameClock.ManualFrameClock()
        coordinator = AnimatedTextEditorCoordinator(
            context,
            bridge,
            ManualAnimationTimeSource(),
            TransactionIdSource(),
            WindowDisplayFrameClock(manualFrameClock),
        )
    }

    @After
    fun tearDown() {
        try {
            coordinator.releaseHost()
        } catch (_: Exception) {}
        try {
            serviceHolder.close()
        } catch (_: Exception) {}
        try {
            testDir.deleteRecursively()
        } catch (_: Exception) {}
    }

    @Test
    fun animatedTextField_insertTextAtCursor_appendsAtEnd() {
        var currentValue by mutableStateOf("hello")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "test-field",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("test-field")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) {
                it(AnnotatedString("X"))
            }

        composeTestRule.waitForIdle()
        assertEquals("helloX", currentValue)
    }

    @Test
    fun animatedTextField_setText_replacesAll() {
        var currentValue by mutableStateOf("old")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "test-field",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("test-field")
            .performSemanticsAction(SemanticsActions.SetText) {
                it(AnnotatedString("new"))
            }

        composeTestRule.waitForIdle()
        assertEquals("new", currentValue)
    }

    @Test
    fun animatedTextField_setSelection_updatesSelectionRange() {
        var currentValue by mutableStateOf("hello")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "test-field",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("test-field")
            .performSemanticsAction(SemanticsActions.SetSelection) {
                it(1, 3, false)
            }

        composeTestRule.waitForIdle()
        composeTestRule.onNodeWithTag("test-field").assert(
            SemanticsMatcher.expectValue(
                SemanticsProperties.TextSelectionRange,
                TextRange(1, 3)
            )
        )
    }

    @Test
    fun animatedTextField_disabled_noEditableActions() {
        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "disabled-field",
                    value = "readonly",
                    onValueChange = {},
                    onCommit = {},
                    enabled = false,
                )
            }
        }

        val node = composeTestRule.onNodeWithTag("disabled-field")
        assertFalse(
            node.fetchSemanticsNode().config.contains(SemanticsActions.SetText)
        )
        assertFalse(
            node.fetchSemanticsNode().config.contains(SemanticsActions.InsertTextAtCursor)
        )
        assertFalse(
            node.fetchSemanticsNode().config.contains(SemanticsActions.SetSelection)
        )
    }

    @Test
    fun animatedTextArea_insertTextAtCursor_appendsAtEnd() {
        var currentValue by mutableStateOf("hello")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextArea(
                    targetId = "test-area",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("test-area")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) {
                it(AnnotatedString("X"))
            }

        composeTestRule.waitForIdle()
        assertEquals("helloX", currentValue)
    }

    @Test
    fun animatedTextArea_setText_replacesAll() {
        var currentValue by mutableStateOf("old")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextArea(
                    targetId = "test-area",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("test-area")
            .performSemanticsAction(SemanticsActions.SetText) {
                it(AnnotatedString("new"))
            }

        composeTestRule.waitForIdle()
        assertEquals("new", currentValue)
    }

    @Test
    fun animatedInlineText_insertTextAtCursor_appendsAtEnd() {
        var currentValue by mutableStateOf("label")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedInlineText(
                    targetId = "test-inline",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("test-inline")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) {
                it(AnnotatedString("X"))
            }

        composeTestRule.waitForIdle()
        assertEquals("labelX", currentValue)
    }

    @Test
    fun animatedInlineText_setText_replacesAll() {
        var currentValue by mutableStateOf("old")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedInlineText(
                    targetId = "test-inline",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("test-inline")
            .performSemanticsAction(SemanticsActions.SetText) {
                it(AnnotatedString("new"))
            }

        composeTestRule.waitForIdle()
        assertEquals("new", currentValue)
    }

    @Test
    fun animatedTextField_emoji_insertTextAtCursor_utf8Correct() {
        var currentValue by mutableStateOf("abc")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "test-field",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("test-field")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) {
                it(AnnotatedString("🙂"))
            }

        composeTestRule.waitForIdle()
        assertEquals("abc🙂", currentValue)
        val utf8Len = currentValue.toByteArray(Charsets.UTF_8).size
        assertEquals(7, utf8Len)
    }

    @Test
    fun animatedTextField_setSelection_clampedToBounds() {
        var currentValue by mutableStateOf("hi")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "test-field",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("test-field")
            .performSemanticsAction(SemanticsActions.SetSelection) {
                it(-1, 100, false)
            }

        composeTestRule.waitForIdle()
        composeTestRule.onNodeWithTag("test-field").assert(
            SemanticsMatcher.expectValue(
                SemanticsProperties.TextSelectionRange,
                TextRange(0, 2)
            )
        )
    }

    @Test
    fun animatedTextField_externalValueUpdate_resetsSelection() {
        var currentValue by mutableStateOf("hello")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "test-field",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("test-field")
            .performSemanticsAction(SemanticsActions.SetSelection) {
                it(2, 4, false)
            }
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("test-field").assert(
            SemanticsMatcher.expectValue(
                SemanticsProperties.TextSelectionRange,
                TextRange(2, 4)
            )
        )

        currentValue = "hi"
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("test-field").assert(
            SemanticsMatcher.expectValue(
                SemanticsProperties.TextSelectionRange,
                TextRange(2)
            )
        )
    }
}
