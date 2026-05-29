package com.xiwei.writerapp.data

class NativeStatusBridge internal constructor(private val nativeBridge: NativeCoreBridge) {
    val isLoaded: Boolean get() = nativeBridge.isLoaded
    val workspaceDir: String get() = nativeBridge.workspaceDirPath()

    fun validateWorkspace(): Boolean = nativeBridge.validateWorkspace()
    fun aiAvailable(): Boolean = nativeBridge.aiAvailable()
}
