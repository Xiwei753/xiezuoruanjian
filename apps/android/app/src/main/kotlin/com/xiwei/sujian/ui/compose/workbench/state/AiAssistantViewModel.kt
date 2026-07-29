package com.xiwei.sujian.ui.compose.workbench.state

import android.app.Application
import androidx.compose.runtime.mutableStateOf
import androidx.lifecycle.AndroidViewModel

data class ChatMessage(
    val role: String,
    val text: String,
)

class AiAssistantViewModel(
    application: Application,
) : AndroidViewModel(application) {

    var messageList = mutableStateOf<List<ChatMessage>>(emptyList())
        private set

    var currentInput = mutableStateOf("")
        private set

    var currentProjectId = mutableStateOf<String?>(null)
        private set

    var currentVolumeId = mutableStateOf<String?>(null)
        private set

    var currentChapterId = mutableStateOf<String?>(null)
        private set

    fun setCurrentIds(projectId: String?, volumeId: String?, chapterId: String?) {
        currentProjectId.value = projectId
        currentVolumeId.value = volumeId
        currentChapterId.value = chapterId
    }

    fun updateInput(text: String) {
        currentInput.value = text
    }

    fun sendMessage() {
        val text = currentInput.value.trim()
        if (text.isEmpty()) return
        messageList.value = messageList.value + ChatMessage(role = "user", text = text)
        currentInput.value = ""
        messageList.value = messageList.value + ChatMessage(
            role = "assistant",
            text = "This is a placeholder AI response. AI integration coming soon."
        )
    }
}
