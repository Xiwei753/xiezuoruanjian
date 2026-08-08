package com.xiwei.sujian.app.navigation.model

import com.xiwei.sujian.app.layout.model.ShellMode

enum class ScreenRole {
    Home,
    ProjectList,
    ProjectWorkspace,
    Writing,
    StarMap,
    Stats,
    Settings,
    Sync,
}

enum class ActionRole {
    Back,
    Save,
    CreateProject,
    CreateVolume,
    CreateChapter,
    Delete,
    Rename,
    Settings,
    Sync,
    Search,
    Sort,
}

enum class ActionPlacement {
    TopLeading,
    TopTrailing,
    Floating,
    BottomBar,
    ContextMenu,
    SidePanel,
    Navigation,
    ListHeader,
    ItemTrailing,
    EmptyState,
}

enum class PaneRole {
    PrimaryList,
    Detail,
    Editor,
    Inspector,
    Drawer,
    Supporting,
}

data class ActionSlot(
    val actionId: String,
    val role: ActionRole,
    val placement: ActionPlacement,
    val visibleIn: List<ShellMode>,
    val requiresConfirmation: Boolean,
)

data class ScreenPolicy(
    val screenRole: ScreenRole,
    val actionSlots: List<ActionSlot>,
)
