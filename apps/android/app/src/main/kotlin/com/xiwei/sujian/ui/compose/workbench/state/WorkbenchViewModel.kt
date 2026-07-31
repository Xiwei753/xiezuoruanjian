package com.xiwei.sujian.ui.compose.workbench.state

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

class WorkbenchViewModel(
    application: Application,
) : AndroidViewModel(application) {

    var layoutState by mutableStateOf(WorkbenchReducer.computeDefaultLayout())
        private set

    private var repository: WorkbenchLayoutStore? = null
    private var currentStorageKey: LayoutStorageKey? = null
    private var pendingResizeJob: Job? = null
    private val switchMutex = Mutex()
    private var isInitialized = false
    private var lastMaxWidthDp: Float = 0f
    private var lastMaxHeightDp: Float = 0f
    private var activeSwitchCount = 0
    private var switchGeneration = 0L
    private val pendingActions = ArrayDeque<WorkbenchAction>()

    private val isSwitching: Boolean
        get() = activeSwitchCount > 0

    fun initialize(repository: WorkbenchLayoutStore, storageKey: LayoutStorageKey) {
        if (isInitialized) {
            onWindowBucketChanged(storageKey)
            return
        }
        isInitialized = true
        this.repository = repository
        val generation = ++switchGeneration
        activeSwitchCount++
        viewModelScope.launch {
            try {
                switchStorageKey(storageKey, generation)
            } finally {
                activeSwitchCount--
            }
        }
    }

    fun dispatch(action: WorkbenchAction) {
        if (isSwitching) {
            pendingActions.addLast(action)
            return
        }
        layoutState = WorkbenchReducer.reduce(layoutState, action)
        if (action !is WorkbenchAction.ResizePanel && action !is WorkbenchAction.ResizePanelDelta && action !is WorkbenchAction.MoveFloatingPanel && action !is WorkbenchAction.ClampFloatingPanels) {
            schedulePersist()
        }
    }

    fun dispatchDeferredPersist(action: WorkbenchAction) {
        if (isSwitching) {
            pendingActions.addLast(action)
            return
        }
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
        val generation = ++switchGeneration
        activeSwitchCount++
        switchJob = viewModelScope.launch {
            try {
                switchStorageKey(newKey, generation)
            } finally {
                activeSwitchCount--
            }
        }
    }

    fun onWindowSizeChanged(maxWidthDp: Float, maxHeightDp: Float) {
        lastMaxWidthDp = maxWidthDp
        lastMaxHeightDp = maxHeightDp
        dispatch(WorkbenchAction.ClampFloatingPanels(maxWidthDp, maxHeightDp))
        pendingResizeJob?.cancel()
        pendingResizeJob = viewModelScope.launch {
            kotlinx.coroutines.delay(500)
            persistLayout()
        }
    }

    private suspend fun switchStorageKey(newKey: LayoutStorageKey, generation: Long) {
        val repo = repository ?: return
        switchMutex.withLock {
            if (generation != switchGeneration) return@withLock
            val oldKey = currentStorageKey
            pendingResizeJob?.cancel()
            pendingResizeJob = null
            if (oldKey != newKey) {
                if (oldKey != null) {
                    val snapshot = layoutState
                    repo.saveLayout(oldKey, snapshot)
                }
                val saved = repo.loadLayout(newKey)
                if (generation != switchGeneration) return@withLock
                var newState = saved ?: WorkbenchReducer.computeDefaultLayout()
                if (lastMaxWidthDp > 0f && lastMaxHeightDp > 0f) {
                    newState = WorkbenchReducer.reduce(newState, WorkbenchAction.ClampFloatingPanels(lastMaxWidthDp, lastMaxHeightDp))
                }
                layoutState = newState
                currentStorageKey = newKey
            }
            while (pendingActions.isNotEmpty()) {
                while (pendingActions.isNotEmpty()) {
                    layoutState = WorkbenchReducer.reduce(layoutState, pendingActions.removeFirst())
                }
                repo.saveLayout(newKey, layoutState)
            }
        }
    }

    private fun schedulePersist() {
        viewModelScope.launch { persistLayout() }
    }

    private suspend fun persistLayout() {
        switchMutex.withLock {
            val repo = repository ?: return@withLock
            val key = currentStorageKey ?: return@withLock
            val snapshot = layoutState
            repo.saveLayout(key, snapshot)
        }
    }
}
