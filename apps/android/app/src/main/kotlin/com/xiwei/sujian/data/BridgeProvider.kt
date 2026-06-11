package com.xiwei.sujian.data

import android.content.Context

object BridgeProvider {
    @Volatile
    private var appServiceInstance: AppServiceBridge? = null

    private fun getAppServiceBridge(context: Context): AppServiceBridge {
        return appServiceInstance ?: synchronized(this) {
            appServiceInstance ?: AppServiceBridge(
                WorkspaceManager.getWorkspaceDir(context.applicationContext).absolutePath
            ).also { appServiceInstance = it }
        }
    }

    fun getWorkspaceBridge(context: Context): WorkspaceBridge = WorkspaceBridge(getAppServiceBridge(context))
    fun getWritingBridge(context: Context): WritingBridge = WritingBridge(getAppServiceBridge(context))
    fun getStatsBridge(context: Context): StatsBridge = StatsBridge(getAppServiceBridge(context))
    fun getStarmapBridge(context: Context): StarMapBridge = StarMapBridge(getAppServiceBridge(context))
    fun getMindMapBridge(context: Context): MindMapBridge = MindMapBridge(getAppServiceBridge(context)) // LEGACY: 仅迁移兼容
    fun getSettingsBridge(context: Context): SettingsBridge = SettingsBridge(getAppServiceBridge(context))
    fun getSyncBridge(context: Context): SyncBridge = SyncBridge(getAppServiceBridge(context))
    fun getActionBridge(context: Context): ActionBridge = ActionBridge(getAppServiceBridge(context))
    fun getAiStatus(context: Context): Boolean = getAppServiceBridge(context).aiAvailable()
}
