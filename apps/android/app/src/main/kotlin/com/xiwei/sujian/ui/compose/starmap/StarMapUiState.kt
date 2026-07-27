package com.xiwei.sujian.ui.compose.starmap

import com.xiwei.sujian.model.StarMapData
import com.xiwei.sujian.model.StarMapMeta

internal data class StarMapListUiState(
    val starMaps: List<StarMapMeta> = emptyList(),
    val isLoading: Boolean = true
)

internal data class StarMapEditorUiState(
    val starMapData: StarMapData? = null,
    val isLoading: Boolean = true,
    val selectedNodeId: String? = null,
    val editingNodeId: String? = null,
    val lastError: String? = null,
    val operationInProgress: Boolean = false
)

sealed class StarMapOperationResult {
    data class Success(val data: Any? = null) : StarMapOperationResult()
    data class Error(val message: String) : StarMapOperationResult()
}
