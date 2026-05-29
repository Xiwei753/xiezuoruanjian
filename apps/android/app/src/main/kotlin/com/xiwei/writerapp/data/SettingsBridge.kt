package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.LocalSettings
import com.xiwei.writerapp.model.SyncableSettings

class SettingsBridge internal constructor(private val nativeBridge: NativeCoreBridge) {
    fun getLocalSettings(): BridgeResult<LocalSettings?> = nativeBridge.getLocalSettings().toBridgeResult()
    fun saveLocalSettings(settings: LocalSettings): BridgeResult<Boolean> =
        nativeBridge.saveLocalSettings(settings).toBridgeResult()
    fun getSyncableSettings(): BridgeResult<SyncableSettings?> = nativeBridge.getSyncableSettings().toBridgeResult()
    fun saveSyncableSettings(settings: SyncableSettings): BridgeResult<Boolean> =
        nativeBridge.saveSyncableSettings(settings).toBridgeResult()
}
