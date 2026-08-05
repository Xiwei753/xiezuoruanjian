package com.xiwei.sujian.ui.phone.portrait

import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue

@Stable
class PhoneWorkspaceNavigationState {
    var currentLocation by mutableStateOf<WorkspaceLocation>(WorkspaceLocation.ProjectList)
        private set

    fun navigateToProjectList() {
        currentLocation = WorkspaceLocation.ProjectList
    }

    fun navigateToChapterTree(projectId: String) {
        currentLocation = WorkspaceLocation.ChapterTree(projectId)
    }

    fun navigateToEditor(projectId: String, volumeId: String, chapterId: String) {
        currentLocation = WorkspaceLocation.Editor(projectId, volumeId, chapterId)
    }

    fun back(): Boolean {
        val location = currentLocation
        return when (location) {
            is WorkspaceLocation.Editor -> {
                currentLocation = WorkspaceLocation.ChapterTree(location.projectId)
                true
            }
            is WorkspaceLocation.ChapterTree -> {
                currentLocation = WorkspaceLocation.ProjectList
                true
            }
            is WorkspaceLocation.ProjectList -> false
        }
    }
}
