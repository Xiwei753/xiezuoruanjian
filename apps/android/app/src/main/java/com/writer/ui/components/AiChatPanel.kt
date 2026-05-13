package com.writer.ui.components

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.spring
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

enum class ContextToggle {
    CURRENT_TEXT, SELECTED_CHAPTERS, FULL_BOOK, NO_CONTEXT
}

@Composable
fun OmniscientAiChatPanel(
    messages: List<AiMessage>,
    selectedContext: ContextToggle,
    onContextSelected: (ContextToggle) -> Unit,
    onSendMessage: (String) -> Unit
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(Color(0xFF1E1E1E))
    ) {
        // Context Toggles (Beautifully Animated Spring Chips)
        LazyRow(
            modifier = Modifier.padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            items(ContextToggle.values()) { toggle ->
                FilterChip(
                    selected = toggle == selectedContext,
                    onClick = { onContextSelected(toggle) },
                    label = { Text(toggle.name.replace("_", " ")) },
                    // In a full app, we would use animateColorAsState with spring() here
                )
            }
        }

        // Chat History
        LazyColumn(
            modifier = Modifier
                .weight(1f)
                .padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            items(messages) { msg ->
                ChatBubble(message = msg)
            }
        }

        // Input Area
        var textState by remember { mutableStateOf("") }
        Row(modifier = Modifier.padding(16.dp)) {
            TextField(
                value = textState,
                onValueChange = { textState = it },
                modifier = Modifier.weight(1f),
                placeholder = { Text("Ask DeepSeek...") }
            )
            Button(onClick = { onSendMessage(textState); textState = "" }) {
                Text("Send")
            }
        }
    }
}

data class AiMessage(
    val content: String,
    val reasoningContent: String? = null,
    val isUser: Boolean = false
)

@Composable
fun ChatBubble(message: AiMessage) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = if (message.isUser) Alignment.End else Alignment.Start
    ) {
        if (!message.isUser && !message.reasoningContent.isNullOrEmpty()) {
            var expanded by remember { mutableStateOf(false) }

            Column(modifier = Modifier
                .padding(bottom = 4.dp)
                .background(Color(0xFF2A2A2A), RoundedCornerShape(8.dp))
                .clickable { expanded = !expanded }
                .padding(8.dp)
            ) {
                Text(
                    text = "Thinking...",
                    color = Color.Gray,
                    fontSize = 12.sp,
                    modifier = Modifier.alpha(0.8f)
                )

                // Collapsible <think> streaming block
                AnimatedVisibility(
                    visible = expanded,
                    enter = expandVertically(animationSpec = spring()),
                    exit = shrinkVertically(animationSpec = spring())
                ) {
                    Text(
                        text = message.reasoningContent,
                        color = Color.LightGray,
                        fontSize = 12.sp,
                        modifier = Modifier.padding(top = 4.dp)
                    )
                }
            }
        }

        Box(
            modifier = Modifier
                .background(
                    if (message.isUser) Color(0xFF007AFF) else Color(0xFF333333),
                    RoundedCornerShape(12.dp)
                )
                .padding(12.dp)
        ) {
            Text(text = message.content, color = Color.White)
        }
    }
}