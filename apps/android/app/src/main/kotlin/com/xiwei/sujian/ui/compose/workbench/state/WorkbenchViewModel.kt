package com.xiwei.sujian.ui.compose.workbench.state

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext

class WorkbenchViewModel(
    application: Application,
) : AndroidViewModel(application) {

    var layoutState by mutableStateOf(WorkbenchReducer.computeDefaultLayout())
        private set

    private var repository: WorkbenchLayoutRepository? = null
    private var currentStorageKey: LayoutStorageKey? = null
    private var pendingResizeJob: Job? = null
    private val switchMutex = Mutex()
    private var isInitialized = false

    fun initialize(repository: WorkbenchLayoutRepository, storageKey: LayoutStorageKey) {
        if (isInitialized) {
            onWindowBucketChanged(storageKey)
            return
        }
        isInitialized = true
        this.repository = repository
        viewModelScope.launch {
            switchStorageKey(storageKey)
        }
    }

    fun dispatch(action: WorkbenchAction) {
        layoutState = WorkbenchReducer.reduce(layoutState, action)
        if (action !is WorkbenchAction.ResizePanel && action !is WorkbenchAction.MoveFloatingPanel) {
            schedulePersist()
        }
    }

    fun dispatchDeferredPersist(action: WorkbenchAction) {
        layoutState = WorkbenchReducer.reduce(layoutState, action)
        pendingResizeJob?.cancel()
        pendingResizeJob = viewModelScope.launch {
            kotlinx.coroutines.delay(300)
            persistLayout()
        }
    }

    private var switchJob: Job? = null

    fun onWindowBucketChanged(newKey: LayoutStorageKey) {
        switchJob?.cancel()
        switchJob = viewModelScope.launch {
            switchStorageKey(newKey)
        }
    }

    fun onWindowSizeChanged(maxWidthDp: Float, maxHeightDp: Float) {
        dispatch(WorkbenchAction.ClampFloatingPanels(maxWidthDp, maxHeightDp))
    }

    private suspend fun switchStorageKey(newKey: LayoutStorageKey) {
        val repo = repository ?: return
        switchMutex.withLock {
            val oldKey = currentStorageKey
            if (oldKey == newKey) return
            pendingResizeJob?.cancel()
            pendingResizeJob = null
            if (oldKey != null) {
                val snapshot = layoutState
                withContext(Dispatchers.IO) { repo.saveLayout(oldKey, snapshot) }
            }
            currentStorageKey = newKey
            val saved = withContext(Dispatchers.IO) { repo.loadLayout(newKey) }
            layoutState = saved ?: WorkbenchReducer.computeDefaultLayout()
        }
    }

    private fun schedulePersist() {
        viewModelScope.launch { persistLayout() }
    }

    private suspend fun persistLayout() {
        val repo = repository ?: return
        val key = currentStorageKey ?: return
        withContext(Dispatchers.IO) { repo.saveLayout(key, layoutState) }
    }
}
