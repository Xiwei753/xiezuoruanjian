package com.xiwei.writerapp.data

import android.content.Context

object BridgeProvider {
    @Volatile
    private var instance: NativeCoreBridge? = null

    private fun getNativeBridge(context: Context): NativeCoreBridge {
        return instance ?: synchronized(this) {
            instance ?: NativeCoreBridge(context.applicationContext).also { instance = it }
        }
    }

    fun getWorkspaceBridge(context: Context): WorkspaceBridge = WorkspaceBridge(getNativeBridge(context))
    fun getWritingBridge(context: Context): WritingBridge = WritingBridge(getNativeBridge(context))
    fun getStatsBridge(context: Context): StatsBridge = StatsBridge(getNativeBridge(context))
    fun getStarmapBridge(context: Context): StarMapBridge = StarMapBridge(getNativeBridge(context))
    fun getActionBridge(context: Context): ActionBridge = ActionBridge(getNativeBridge(context))
    fun getSettingsBridge(context: Context): SettingsBridge = SettingsBridge(getNativeBridge(context))
    fun getSyncBridge(context: Context): SyncBridge = SyncBridge(getNativeBridge(context))

    fun getNativeStatusBridge(context: Context): NativeStatusBridge = NativeStatusBridge(getNativeBridge(context))
}
