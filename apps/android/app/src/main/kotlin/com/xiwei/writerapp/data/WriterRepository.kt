package com.xiwei.writerapp.data

import android.content.Context

class WriterRepository private constructor(context: Context) {
    private val workspacePath = WorkspaceManager.getWorkspaceDir(context).absolutePath
    val appService = AppServiceBridge(workspacePath)

    val workspace = WorkspaceBridge(appService)
    val writing = WritingBridge(appService)
    val settings = SettingsBridge(appService)
    val sync = SyncBridge(appService)
    val stats = StatsBridge(appService)
    val mindMap = MindMapBridge(appService)
    val starMap = StarMapBridge(appService)


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
