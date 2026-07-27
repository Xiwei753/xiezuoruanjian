package com.xiwei.sujian.ui.compose.starmap

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import com.xiwei.sujian.designsystem.component.SujianDialog
import com.xiwei.sujian.designsystem.component.SujianIconButton
import com.xiwei.sujian.designsystem.component.SujianTextButton
import com.xiwei.sujian.designsystem.icon.SujianIcons
import com.xiwei.sujian.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.editor.v2.compose.AnimatedTextArea
import com.xiwei.sujian.editor.v2.compose.AnimatedTextField
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.R
import com.xiwei.sujian.model.StarMapGraphNode
import com.xiwei.sujian.model.StarMapNodeKind

@Composable
internal fun StarMapCreateDialog(
    coordinator: AnimatedTextEditorCoordinator,
    onConfirm: (title: String, description: String) -> Unit,
    onDismiss: () -> Unit
) {
    val dims = LocalSujianDimensions.current
    var title by remember { mutableStateOf("") }
    var description by remember { mutableStateOf("") }
    SujianDialog(
        onDismissRequest = {
            coordinator.cancelActiveEdit()
            onDismiss()
        },
        title = stringResource(id = R.string.starmap_create_new),
        confirmText = stringResource(id = R.string.action_create),
        onConfirm = {
            coordinator.commitActiveEdit()
            val t = coordinator.lastCommittedText?.trim() ?: title.trim()
            if (t.isNotBlank()) {
                onConfirm(t, description.trim())
                onDismiss()
            }
        },
        dismissText = stringResource(id = R.string.action_cancel),
        onDismiss = {
            coordinator.cancelActiveEdit()
            onDismiss()
        },
        body = {
            Column {
                AnimatedTextField(
                    targetId = "starmap-title:new",
                    value = title,
                    onValueChange = { title = it },
                    onCommit = { },
                    label = { Text(stringResource(id = R.string.starmap_hint_title)) },
                    singleLine = true
                )
                Spacer(modifier = Modifier.height(dims.space8))
                AnimatedTextArea(
                    targetId = "starmap-description:new",
                    value = description,
                    onValueChange = { description = it },
                    onCommit = { },
                    label = { Text(stringResource(id = R.string.starmap_hint_description)) },
                    minLines = 2,
                    maxLines = 3
                )
            }
        }
    )
}

@Composable
internal fun StarMapAddNodeDialog(
    coordinator: AnimatedTextEditorCoordinator,
    onConfirm: (title: String, kind: StarMapNodeKind) -> Unit,
    onDismiss: () -> Unit
) {
    val dims = LocalSujianDimensions.current
    var nodeTitle by remember { mutableStateOf("") }
    var nodeKind by remember { mutableStateOf(StarMapNodeKind.Note) }
    SujianDialog(
        onDismissRequest = {
            coordinator.cancelActiveEdit()
            onDismiss()
        },
        title = stringResource(id = R.string.starmap_add_node),
        confirmText = stringResource(id = R.string.starmap_action_add),
        onConfirm = {
            coordinator.commitActiveEdit()
            val t = coordinator.lastCommittedText?.trim() ?: nodeTitle.trim()
            if (t.isNotBlank()) {
                onConfirm(t, nodeKind)
            }
        },
        dismissText = stringResource(id = R.string.action_cancel),
        onDismiss = {
            coordinator.cancelActiveEdit()
            onDismiss()
        },
        body = {
            Column {
                AnimatedTextField(
                    targetId = "starmap-node-title:new",
                    value = nodeTitle,
                    onValueChange = { nodeTitle = it },
                    onCommit = { },
                    label = { Text(stringResource(id = R.string.starmap_hint_title)) },
                    singleLine = true
                )
                Spacer(modifier = Modifier.height(dims.space8))
                Row(horizontalArrangement = Arrangement.spacedBy(dims.space4)) {
                    StarMapNodeKind.entries.take(6).forEach { kind ->
                        SujianTextButton(
                            text = kind.name,
                            onClick = { nodeKind = kind },
                        )
                    }
                }
            }
        }
    )
}

@Composable
internal fun StarMapAddEdgeDialog(
    nodes: List<StarMapGraphNode>,
    onConfirm: (fromNodeId: String, toNodeId: String) -> Unit,
    onDismiss: () -> Unit
) {
    val dims = LocalSujianDimensions.current
    var fromNodeId by remember { mutableStateOf(nodes.firstOrNull()?.id ?: "") }
    var toNodeId by remember { mutableStateOf(nodes.drop(1).firstOrNull()?.id ?: "") }

    SujianDialog(
        onDismissRequest = onDismiss,
        title = stringResource(id = R.string.starmap_add_edge),
        confirmText = stringResource(id = R.string.starmap_action_add),
        onConfirm = {
            if (fromNodeId.isNotBlank() && toNodeId.isNotBlank() && fromNodeId != toNodeId) {
                onConfirm(fromNodeId, toNodeId)
            }
        },
        dismissText = stringResource(id = R.string.action_cancel),
        onDismiss = onDismiss,
        body = {
            Column {
                Text(stringResource(id = R.string.starmap_from_node), style = MaterialTheme.typography.bodySmall)
                LazyColumn(modifier = Modifier.height(dims.dialogListHeight)) {
                    items(nodes) { node ->
                        SujianTextButton(
                            text = node.title,
                            onClick = { fromNodeId = node.id },
                            modifier = Modifier.fillMaxWidth()
                        )
                    }
                }
                Spacer(modifier = Modifier.height(dims.space8))
                Text(stringResource(id = R.string.starmap_to_node), style = MaterialTheme.typography.bodySmall)
                LazyColumn(modifier = Modifier.height(dims.dialogListHeight)) {
                    items(nodes) { node ->
                        SujianTextButton(
                            text = node.title,
                            onClick = { toNodeId = node.id },
                            modifier = Modifier.fillMaxWidth()
                        )
                    }
                }
            }
        }
    )
}

@Composable
internal fun NodeEditPanel(
    node: StarMapGraphNode,
    coordinator: AnimatedTextEditorCoordinator,
    onUpdate: (title: String, kind: StarMapNodeKind) -> Unit,
    onDelete: () -> Unit,
    onDismiss: () -> Unit
) {
    val dims = LocalSujianDimensions.current
    var editTitle by remember { mutableStateOf(node.title) }
    var editKind by remember { mutableStateOf(node.kind) }

    SujianDialog(
        onDismissRequest = {
            coordinator.cancelActiveEdit()
            onDismiss()
        },
        title = stringResource(id = R.string.starmap_edit_node),
        confirmText = stringResource(id = R.string.action_save),
        onConfirm = {
            coordinator.commitActiveEdit()
            val finalTitle = coordinator.lastCommittedText?.trim() ?: editTitle.trim()
            if (finalTitle.isNotBlank()) {
                onUpdate(finalTitle, editKind)
            }
        },
        dismissText = stringResource(id = R.string.action_cancel),
        onDismiss = {
            coordinator.cancelActiveEdit()
            onDismiss()
        },
        body = {
            Column {
                AnimatedTextField(
                    targetId = "starmap-node-title:edit",
                    value = editTitle,
                    onValueChange = { editTitle = it },
                    onCommit = { },
                    label = { Text(stringResource(id = R.string.starmap_hint_title)) },
                    singleLine = true
                )
                Spacer(modifier = Modifier.height(dims.space8))
                Row(horizontalArrangement = Arrangement.spacedBy(dims.space4)) {
                    StarMapNodeKind.entries.take(6).forEach { kind ->
                        SujianTextButton(
                            text = kind.name,
                            onClick = { editKind = kind },
                        )
                    }
                }
                Spacer(modifier = Modifier.height(dims.space16))
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.End
                ) {
                    SujianIconButton(
                        onClick = onDelete,
                        icon = SujianIcons.Delete,
                        contentDescription = stringResource(id = R.string.starmap_delete_node),
                    )
                }
            }
        }
    )
}
