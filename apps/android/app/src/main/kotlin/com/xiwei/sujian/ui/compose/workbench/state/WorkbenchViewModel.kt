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
            switchStorageKey(storageKey)
            return
        }
        isInitialized = true
        this.repository = repository
        this.currentStorageKey = storageKey
        viewModelScope.launch {
            val saved = withContext(Dispatchers.IO) {
                repository.loadLayout(storageKey)
            }
            if (saved != null) {
                layoutState = saved
            }
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

    fun onWindowBucketChanged(newKey: LayoutStorageKey) {
        switchStorageKey(newKey)
    }

    private fun switchStorageKey(newKey: LayoutStorageKey) {
        val oldKey = currentStorageKey
        if (oldKey == newKey) return
        val repo = repository ?: return
        viewModelScope.launch {
            switchMutex.withLock {
                if (oldKey != null) {
                    withContext(Dispatchers.IO) { repo.saveLayout(oldKey, layoutState) }
                }
                currentStorageKey = newKey
                val saved = withContext(Dispatchers.IO) { repo.loadLayout(newKey) }
                layoutState = saved ?: WorkbenchReducer.computeDefaultLayout()
            }
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
