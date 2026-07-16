package com.xiwei.sujian.data

import android.content.Context

object BridgeProvider {
    @Volatile
    private var appServiceInstance: AppServiceBridge? = null

    fun getAppServiceBridge(context: Context): AppServiceBridge {
        return appServiceInstance ?: synchronized(this) {
            appServiceInstance ?: AppServiceBridge(
                WorkspaceManager.getWorkspaceDir(context.applicationContext).absolutePath
            ).also { appServiceInstance = it }
        }
    }

    fun getWorkspaceBridge(context: Context): WorkspaceBridge = WorkspaceBridge(getAppServiceBridge(context))
    fun getWritingBridge(context: Context): WritingBridge = WritingBridge(getAppServiceBridge(context))
    fun getStatsBridge(context: Context): StatsBridge = getAppServiceBridge(context).statsBridge
    fun getStarmapBridge(context: Context): StarMapBridge = getAppServiceBridge(context).starMapBridge
    fun getSettingsBridge(context: Context): SettingsBridge = getAppServiceBridge(context).settingsBridge
    fun getSyncBridge(context: Context): SyncBridge = getAppServiceBridge(context).syncBridge
    fun getActionBridge(context: Context): ActionBridge = ActionBridge(getAppServiceBridge(context))
    fun getLayoutPolicyBridge(context: Context): LayoutPolicyBridge = getAppServiceBridge(context).layoutPolicyBridge
    fun getScreenPolicyBridge(context: Context): ScreenPolicyBridge = ScreenPolicyBridge(getAppServiceBridge(context))
    fun getEditorAnimationBridge(context: Context): EditorAnimationBridge = getAppServiceBridge(context).editorAnimationBridge
    fun getAiStatus(context: Context): Boolean = getAppServiceBridge(context).aiAvailable()
}
