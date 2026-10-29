package com.xiwei.sujian.feature.editor.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.feature.editor.presentation.AnnotationCoordinatorState

@Composable
fun AnnotationSheet(
    state: AnnotationCoordinatorState,
    onDismiss: () -> Unit,
    onTagToggled: (tagId: String, selected: Boolean) -> Unit,
    onCreateTag: (name: String) -> Unit,
    onRegisterName: () -> Unit,
    onNameChanged: (String) -> Unit,
    onSearchChanged: (String) -> Unit,
    onObjectLink: (com.xiwei.sujian.feature.editor.presentation.AnnotationObjectRef) -> Unit,
    onDeleteLink: (linkId: String) -> Unit,
) {
    var newTagName by remember(state.projectId) { mutableStateOf("") }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        LazyColumn(
            modifier = Modifier.fillMaxWidth().heightIn(max = 680.dp).padding(horizontal = 20.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            item {
                Text(
                    text = stringResource(R.string.annotation_sheet_title),
                    style = MaterialTheme.typography.headlineSmall,
                )
                Text(state.targetLabel, style = MaterialTheme.typography.bodySmall)
                if (state.selectedText.isNotBlank()) {
                    Text(
                        text = state.selectedText,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.primary,
                        maxLines = 3,
                    )
                }
                if (state.isLoading) {
                    CircularProgressIndicator(modifier = Modifier.padding(top = 8.dp))
                }
                state.error?.let { error ->
                    Text(error, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                }
            }

            item {
                HorizontalDivider()
                Text(stringResource(R.string.annotation_tags), style = MaterialTheme.typography.titleMedium)
            }
            if (state.tags.isEmpty()) {
                item { Text(stringResource(R.string.annotation_no_tags), style = MaterialTheme.typography.bodySmall) }
            }
            items(state.tags, key = { it.id }) { tag ->
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(
                        checked = tag.id in state.selectedTagIds,
                        onCheckedChange = { onTagToggled(tag.id, it) },
                        enabled = !state.isSaving,
                    )
                    Text(tag.name, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyLarge)
                    tag.color?.let { Text(it, style = MaterialTheme.typography.labelSmall) }
                }
            }
            item {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    OutlinedTextField(
                        value = newTagName,
                        onValueChange = { newTagName = it },
                        modifier = Modifier.weight(1f),
                        label = { Text(stringResource(R.string.annotation_new_tag)) },
                        singleLine = true,
                    )
                    TextButton(
                        enabled = newTagName.isNotBlank() && !state.isSaving,
                        onClick = {
                            onCreateTag(newTagName)
                            newTagName = ""
                        },
                    ) {
                        Text(stringResource(R.string.action_create))
                    }
                }
            }

            item {
                HorizontalDivider()
                Text(stringResource(R.string.annotation_register_name), style = MaterialTheme.typography.titleMedium)
                OutlinedTextField(
                    value = state.nameDraft,
                    onValueChange = onNameChanged,
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text(stringResource(R.string.annotation_name_hint)) },
                    singleLine = true,
                )
                Button(
                    onClick = onRegisterName,
                    enabled = state.nameDraft.isNotBlank() && !state.isSaving,
                    modifier = Modifier.padding(top = 4.dp),
                ) {
                    Text(stringResource(R.string.action_create))
                }
            }

            item {
                HorizontalDivider()
                Text(stringResource(R.string.annotation_links), style = MaterialTheme.typography.titleMedium)
                OutlinedTextField(
                    value = state.linkSearchQuery,
                    onValueChange = onSearchChanged,
                    modifier = Modifier.fillMaxWidth(),
                    label = { Text(stringResource(R.string.annotation_search_links_hint)) },
                    singleLine = true,
                )
            }
            items(
                state.linkSearchResults,
                key = { "${it.ref.projectId}:${it.ref.kind}:${it.ref.objectId}" },
            ) { target ->
                TextButton(
                    onClick = {
                        onObjectLink(target.ref)
                        onSearchChanged("")
                    },
                    enabled = !state.isSaving,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(modifier = Modifier.fillMaxWidth()) {
                        Text(target.title, style = MaterialTheme.typography.bodyMedium)
                        Text(target.subtitle, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
            item {
                Text(stringResource(R.string.annotation_outgoing), style = MaterialTheme.typography.titleSmall)
                if (state.outgoingLinks.isEmpty()) {
                    Text(stringResource(R.string.annotation_no_links), style = MaterialTheme.typography.bodySmall)
                }
            }
            items(state.outgoingLinks, key = { "out:${it.id}" }) { link ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(link.label, modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodySmall)
                    TextButton(onClick = { onDeleteLink(link.id) }, enabled = !state.isSaving) {
                        Text(stringResource(R.string.annotation_remove_link))
                    }
                }
            }
            item {
                HorizontalDivider()
                Text(stringResource(R.string.annotation_backlinks), style = MaterialTheme.typography.titleSmall)
                if (state.backlinks.isEmpty()) {
                    Text(stringResource(R.string.annotation_no_backlinks), style = MaterialTheme.typography.bodySmall)
                }
            }
            items(state.backlinks, key = { "back:${it.id}" }) { link ->
                Text(link.label, style = MaterialTheme.typography.bodySmall)
            }
            item { Spacer(Modifier.padding(bottom = 18.dp)) }
        }
    }
}
