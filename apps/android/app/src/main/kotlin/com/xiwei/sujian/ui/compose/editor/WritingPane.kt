package com.xiwei.sujian.ui.compose.editor

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.host.UniFFIEditorKernelBridge
import com.xiwei.sujian.ui.EditorViewModel
import com.xiwei.sujian.ui.SaveStatus
import com.xiwei.sujian.ui.compose.theme.BindEditorThemeColors

@Composable
fun WritingPane(
    projectId: String,
    volumeId: String,
    chapterId: String,
    chapterTitle: String,
    modifier: Modifier = Modifier
) {
    val context = LocalContext.current
    val viewModel: EditorViewModel = viewModel()
    var editorView by remember { mutableStateOf<SujianEditorView?>(null) }
    var isApplyingExternalContent by remember { mutableStateOf(false) }

    var lastFontSize by remember { mutableStateOf(-1f) }
    var lastLineSpacing by remember { mutableStateOf(-1f) }
    var lastTypingAnimEnabled by remember { mutableStateOf(false) }
    var lastTypingAnimDuration by remember { mutableStateOf(0L) }
    var lastSmoothCursorEnabled by remember { mutableStateOf(false) }
    var lastSmoothCursorDuration by remember { mutableStateOf(0L) }
    var lastAutoIndentEnabled by remember { mutableStateOf(false) }
    var lastAutoIndentWidth by remember { mutableStateOf(0f) }
    var lastCoordinatedAnimEnabled by remember { mutableStateOf(false) }

    LaunchedEffect(projectId, volumeId, chapterId) {
        viewModel.switchChapter(projectId, volumeId, chapterId, chapterTitle)
    }

    val uiState by viewModel.uiState.collectAsState()

    BindEditorThemeColors(editorView)

    editorView?.let { view ->
        val settings = uiState.settings
        if (lastFontSize != settings.fontSize) {
            lastFontSize = settings.fontSize
            view.setFontSize(settings.fontSize)
        }
        if (lastLineSpacing != settings.lineSpacingMultiplier) {
            lastLineSpacing = settings.lineSpacingMultiplier
            view.setLineSpacingMultiplier(settings.lineSpacingMultiplier)
        }
        if (lastTypingAnimEnabled != settings.typingAnimationEnabled || lastTypingAnimDuration != settings.typingAnimationDurationMs) {
            lastTypingAnimEnabled = settings.typingAnimationEnabled
            lastTypingAnimDuration = settings.typingAnimationDurationMs
            view.setTypingAnimationEnabled(settings.typingAnimationEnabled, settings.typingAnimationDurationMs)
        }
        if (lastSmoothCursorEnabled != settings.smoothCursorEnabled || lastSmoothCursorDuration != settings.smoothCursorDurationMs) {
            lastSmoothCursorEnabled = settings.smoothCursorEnabled
            lastSmoothCursorDuration = settings.smoothCursorDurationMs
            view.setSmoothCursorEnabled(settings.smoothCursorEnabled, settings.smoothCursorDurationMs)
        }
        if (lastAutoIndentEnabled != settings.autoIndentEnabled || lastAutoIndentWidth != settings.autoIndentWidth) {
            lastAutoIndentEnabled = settings.autoIndentEnabled
            lastAutoIndentWidth = settings.autoIndentWidth
            view.setAutoIndent(settings.autoIndentEnabled, settings.autoIndentWidth)
        }
        if (lastCoordinatedAnimEnabled != settings.coordinatedTextCursorAnimationEnabled) {
            lastCoordinatedAnimEnabled = settings.coordinatedTextCursorAnimationEnabled
            view.setCoordinatedAnimationEnabled(settings.coordinatedTextCursorAnimationEnabled)
        }
    }

    Column(modifier = modifier.fillMaxSize()) {
        Row(
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            Text(
                chapterTitle,
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.weight(1f)
            )
            val statusText = when (uiState.saveStatus) {
                SaveStatus.Idle -> ""
                SaveStatus.Unsaved -> stringResource(id = R.string.status_unsaved)
                SaveStatus.Saving -> stringResource(id = R.string.status_saving)
                SaveStatus.Saved -> stringResource(id = R.string.status_saved)
                SaveStatus.SaveFailed -> stringResource(id = R.string.status_save_failed)
            }
            if (statusText.isNotEmpty()) {
                Text(statusText, style = MaterialTheme.typography.bodySmall)
            }
        }

        if (uiState.wordCount > 0) {
            Text(
                stringResource(R.string.word_count_format, uiState.wordCount),
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 2.dp)
            )
        }

        if (uiState.loading) {
            Box(modifier = Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
                CircularProgressIndicator()
            }
        } else {
            AndroidView(
                factory = { ctx ->
                    SujianEditorView(ctx).apply {
                        layoutParams = android.widget.FrameLayout.LayoutParams(
                            android.widget.FrameLayout.LayoutParams.MATCH_PARENT,
                            android.widget.FrameLayout.LayoutParams.MATCH_PARENT
                        )
                        editorView = this

                        try {
                            val appServiceBridge = BridgeProvider.getAppServiceBridge(ctx)
                            kernelBridge = UniFFIEditorKernelBridge(appServiceBridge)
                        } catch (_: Exception) { }

                        onContentChanged = { newText ->
                            if (!isApplyingExternalContent) {
                                viewModel.onContentChanged(newText)
                            }
                        }
                    }
                },
                update = { view ->
                    if (view.getText() != uiState.content) {
                        isApplyingExternalContent = true
                        view.setText(uiState.content)
                        isApplyingExternalContent = false
                    }
                },
                modifier = Modifier.weight(1f).fillMaxWidth()
            )
        }
    }
}
