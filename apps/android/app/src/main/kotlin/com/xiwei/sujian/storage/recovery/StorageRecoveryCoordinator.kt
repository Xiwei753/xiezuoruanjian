package com.xiwei.sujian.storage.recovery

import android.content.Context
import android.net.Uri
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * 恢复结果。
 */
sealed class RecoveryResult {
    /** 旧版共享存储迁移成功。 */
    object LegacyImported : RecoveryResult()

    /** Download 镜像恢复成功。 */
    object MirrorRestored : RecoveryResult()

    /** 无可恢复数据。 */
    object NothingToRecover : RecoveryResult()

    /** 恢复失败。 */
    data class Failed(val reason: String) : RecoveryResult()
}

/**
 * StorageRecoveryCoordinator — 存储恢复协调器。
 *
 * 区分两种来源：
 * - Download 镜像（含 `_meta/manifest.json`）→ [ReadableMirrorRestorer]
 * - 旧版共享存储（含 `.git` 或 sujian-git）→ [LegacySharedStorageImporter]
 *
 * 判断依据：读取 URI 根下是否有 `_meta/manifest.json`。manifest 存在走镜像恢复，
 * 否则走旧版迁移。位于 `:app` 的 `storage/recovery` 包，可依赖 :app 的 interop bridge
 * 与 DI，但不放 Composable（UI 接入由上层负责）。
 */
class StorageRecoveryCoordinator(
    private val context: Context,
    private val documentTreeReader: DocumentTreeReader,
    private val appServiceBridge: AppServiceBridge,
    private val changeSink: RecoveryChangeSink,
) {
    private val legacyImporter by lazy { LegacySharedStorageImporter() }
    private val mirrorRestorer by lazy { ReadableMirrorRestorer() }

    suspend fun recoverFromUri(treeUri: Uri): RecoveryResult =
        withContext(Dispatchers.IO) {
            if (hasManifest(treeUri)) {
                when (
                    val result =
                        mirrorRestorer.restore(
                            context,
                            treeUri,
                            documentTreeReader,
                            appServiceBridge,
                            changeSink,
                        )
                ) {
                    is RestoreResult.Success -> RecoveryResult.MirrorRestored
                    is RestoreResult.ManifestMissing -> RecoveryResult.NothingToRecover
                    is RestoreResult.RestoreFailed -> RecoveryResult.Failed(result.reason)
                }
            } else {
                when (val result = legacyImporter.import(context, treeUri, documentTreeReader)) {
                    is ImportResult.Success -> RecoveryResult.LegacyImported
                    is ImportResult.NoLegacyGitFound -> RecoveryResult.NothingToRecover
                    is ImportResult.CopyFailed -> RecoveryResult.Failed(result.reason)
                }
            }
        }

    /**
     * 判断 [treeUri] 根下是否含 `_meta/manifest.json`。
     * 查询失败时返回 false（保守地走旧版迁移路径，由 importer 进一步判断）。
     */
    private fun hasManifest(treeUri: Uri): Boolean =
        try {
            val metaDir =
                documentTreeReader
                    .listChildren(treeUri)
                    .find { it.isDirectory && it.name == META_DIR_NAME }
            if (metaDir == null) {
                false
            } else {
                documentTreeReader.listChildren(metaDir.uri).any { it.name == MANIFEST_FILE_NAME }
            }
        } catch (e: Exception) {
            false
        }

    companion object {
        private const val META_DIR_NAME = "_meta"
        private const val MANIFEST_FILE_NAME = "manifest.json"
    }
}
