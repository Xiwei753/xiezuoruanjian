package com.xiwei.sujian.data

import android.content.Context

class WriterRepository private constructor(context: Context) {
    private val workspacePath = WorkspaceManager.getWorkspaceDir(context).absolutePath
    val appService = AppServiceBridge(workspacePath)

    val workspace = WorkspaceBridge(appService)
    val writing = WritingBridge(appService)
    val settings = appService.settingsBridge
    val sync = appService.syncBridge
    val stats = appService.statsBridge
    val starMap = appService.starMapBridge


    companion object {
        @Volatile
        private var instance: WriterRepository? = null

        fun getInstance(context: Context): WriterRepository {
            return instance ?: synchronized(this) {
                instance ?: WriterRepository(context.applicationContext).also { instance = it }
            }
        }
    }
}
