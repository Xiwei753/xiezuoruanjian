package com.xiwei.sujian.editor

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
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
import com.xiwei.sujian.editor.v2.compose.TextOffsetUtils
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
        try { coordinator.releaseHost() } catch (_: Exception) {}
        try { serviceHolder.close() } catch (_: Exception) {}
        try { testDir.deleteRecursively() } catch (_: Exception) {}
    }

    @Test
    fun field_setSelection_then_insertAtCursor_insertsAtMiddle() {
        var currentValue by mutableStateOf("hello")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "f-mid",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("f-mid")
            .performSemanticsAction(SemanticsActions.SetSelection) { it(2, 2, false) }
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("f-mid")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) { it(AnnotatedString("X")) }
        composeTestRule.waitForIdle()

        assertEquals("heXllo", currentValue)
    }

    @Test
    fun field_setSelectionRange_then_insertAtCursor_replacesSelection() {
        var currentValue by mutableStateOf("hello")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "f-repl",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("f-repl")
            .performSemanticsAction(SemanticsActions.SetSelection) { it(1, 4, false) }
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("f-repl")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) { it(AnnotatedString("XY")) }
        composeTestRule.waitForIdle()

        assertEquals("hXYo", currentValue)
    }

    @Test
    fun field_emoji_setSelection_then_insertAtCursor_utf8ByteOffsetCorrect() {
        var currentValue by mutableStateOf("ab")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "f-emoji",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("f-emoji")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) { it(AnnotatedString("🙂")) }
        composeTestRule.waitForIdle()
        assertEquals("ab🙂", currentValue)

        val utf8LenAfterAppend = currentValue.toByteArray(Charsets.UTF_8).size
        assertEquals("abc=1byte each + emoji=4bytes => 6", 6, utf8LenAfterAppend)

        composeTestRule.onNodeWithTag("f-emoji")
            .performSemanticsAction(SemanticsActions.SetSelection) { it(1, 1, false) }
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("f-emoji")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) { it(AnnotatedString("😀")) }
        composeTestRule.waitForIdle()
        assertEquals("a😀b🙂", currentValue)

        val utf8LenAfterMiddle = currentValue.toByteArray(Charsets.UTF_8).size
        assertEquals("a(1)+😀(4)+b(1)+🙂(4) = 10", 10, utf8LenAfterMiddle)

        val cursorCharIndex = 1 + "😀".length
        val cursorUtf8 = TextOffsetUtils.utf8OffsetForCharIndex(currentValue, cursorCharIndex)
        assertEquals("cursor after a+😀 => byte offset 5", 5, cursorUtf8)
    }

    @Test
    fun field_emoji_setSelectionRange_surrogateSafe_then_insertAtCursor_replacesAndUtf8Correct() {
        var currentValue by mutableStateOf("a🙂b")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "f-emoj-sel",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("f-emoj-sel")
            .performSemanticsAction(SemanticsActions.SetSelection) { it(1, 3, false) }
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("f-emoj-sel").assert(
            SemanticsMatcher.expectValue(SemanticsProperties.TextSelectionRange, TextRange(1, 3))
        )

        composeTestRule.onNodeWithTag("f-emoj-sel")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) { it(AnnotatedString("XY")) }
        composeTestRule.waitForIdle()

        assertEquals("aXYb", currentValue)
        val utf8Len = currentValue.toByteArray(Charsets.UTF_8).size
        assertEquals(4, utf8Len)
    }

    @Test
    fun field_disabled_noEditableActions() {
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
        assertFalse(node.fetchSemanticsNode().config.contains(SemanticsActions.SetText))
        assertFalse(node.fetchSemanticsNode().config.contains(SemanticsActions.InsertTextAtCursor))
        assertFalse(node.fetchSemanticsNode().config.contains(SemanticsActions.SetSelection))
    }

    @Test
    fun area_setSelection_then_insertAtCursor_insertsAtMiddle() {
        var currentValue by mutableStateOf("hello")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextArea(
                    targetId = "a-mid",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("a-mid")
            .performSemanticsAction(SemanticsActions.SetSelection) { it(3, 3, false) }
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("a-mid")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) { it(AnnotatedString("Z")) }
        composeTestRule.waitForIdle()

        assertEquals("helZlo", currentValue)
    }

    @Test
    fun area_setSelectionRange_then_insertAtCursor_replacesSelection() {
        var currentValue by mutableStateOf("hello")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextArea(
                    targetId = "a-repl",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("a-repl")
            .performSemanticsAction(SemanticsActions.SetSelection) { it(1, 4, false) }
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("a-repl")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) { it(AnnotatedString("PQ")) }
        composeTestRule.waitForIdle()

        assertEquals("hPQo", currentValue)
    }

    @Test
    fun inline_setSelection_then_insertAtCursor_insertsAtMiddle() {
        var currentValue by mutableStateOf("label")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedInlineText(
                    targetId = "i-mid",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("i-mid")
            .performSemanticsAction(SemanticsActions.SetSelection) { it(2, 2, false) }
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("i-mid")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) { it(AnnotatedString("W")) }
        composeTestRule.waitForIdle()

        assertEquals("laWbel", currentValue)
    }

    @Test
    fun inline_setSelectionRange_then_insertAtCursor_replacesSelection() {
        var currentValue by mutableStateOf("label")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedInlineText(
                    targetId = "i-repl",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("i-repl")
            .performSemanticsAction(SemanticsActions.SetSelection) { it(1, 3, false) }
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("i-repl")
            .performSemanticsAction(SemanticsActions.InsertTextAtCursor) { it(AnnotatedString("MN")) }
        composeTestRule.waitForIdle()

        assertEquals("lMNel", currentValue)
    }

    @Test
    fun field_setSelection_clampedToBounds() {
        var currentValue by mutableStateOf("hi")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "f-clamp",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("f-clamp")
            .performSemanticsAction(SemanticsActions.SetSelection) { it(-1, 100, false) }
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("f-clamp").assert(
            SemanticsMatcher.expectValue(SemanticsProperties.TextSelectionRange, TextRange(0, 2))
        )
    }

    @Test
    fun field_externalValueUpdate_resetsSelection() {
        var currentValue by mutableStateOf("hello")

        composeTestRule.setContent {
            CompositionLocalProvider(LocalAnimatedTextEditorCoordinator provides coordinator) {
                AnimatedTextField(
                    targetId = "f-reset",
                    value = currentValue,
                    onValueChange = { currentValue = it },
                    onCommit = {},
                )
            }
        }

        composeTestRule.onNodeWithTag("f-reset")
            .performSemanticsAction(SemanticsActions.SetSelection) { it(2, 4, false) }
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("f-reset").assert(
            SemanticsMatcher.expectValue(SemanticsProperties.TextSelectionRange, TextRange(2, 4))
        )

        currentValue = "hi"
        composeTestRule.waitForIdle()

        composeTestRule.onNodeWithTag("f-reset").assert(
            SemanticsMatcher.expectValue(SemanticsProperties.TextSelectionRange, TextRange(2))
        )
    }
}
