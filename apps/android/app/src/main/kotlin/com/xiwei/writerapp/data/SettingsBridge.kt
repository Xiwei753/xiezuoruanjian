package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.LocalSettings
import com.xiwei.writerapp.model.SyncableSettings

class SettingsBridge(private val appService: AppServiceBridge) {
    fun loadLocalSettings(): BridgeResult<LocalSettings> = appService.loadLocalSettings()
    fun getLocalSettings(): BridgeResult<LocalSettings> = loadLocalSettings()
    fun saveLocalSettings(settings: LocalSettings): BridgeResult<Boolean> = appService.saveLocalSettings(settings)
    fun loadSyncableSettings(): BridgeResult<SyncableSettings> = appService.loadSyncableSettings()
    fun getSyncableSettings(): BridgeResult<SyncableSettings> = loadSyncableSettings()
    fun saveSyncableSettings(settings: SyncableSettings): BridgeResult<Boolean> = appService.saveSyncableSettings(settings)
}
