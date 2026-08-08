package com.xiwei.sujian.feature.settings.ui

data class StructuredSyncResult(
    val statusCode: String,
    val messageKey: String,
    val messageArgs: Map<String, String> = emptyMap(),
    val counts: SyncCounts = SyncCounts(),
    val sanitizedDiagnostic: String? = null,
)

data class SyncCounts(
    val uploaded: Int = 0,
    val downloaded: Int = 0,
    val deletedRemote: Int = 0,
    val deletedLocal: Int = 0,
    val conflicts: Int = 0,
    val overwritten: Int = 0,
    val ignored: Int = 0,
)
