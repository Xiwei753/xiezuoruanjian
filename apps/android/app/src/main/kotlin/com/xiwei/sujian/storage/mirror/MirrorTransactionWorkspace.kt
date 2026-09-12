package com.xiwei.sujian.storage.mirror

import android.content.Context
import androidx.core.util.AtomicFile
import com.xiwei.sujian.core.platform.storage.AndroidPrivateDataRoot
import java.io.File
import java.io.IOException

/**
 * MirrorTransactionWorkspace — 镜像事务内部状态的私有文件系统存储。
 *
 * Issue #667：将镜像发布系统的事务中间文件（staging、backup、manifest、journal）
 * 从公开的 `Download/Sujian/` 目录移到应用私有目录 `filesDir/sujian/mirror/`，
 * 使 `Download/Sujian/` 只包含最终用户可见的 `.md` 文件。
 *
 * ## 目录布局
 * - `filesDir/sujian/mirror/staging/<txId>/<relativePath>` — 暂存正文
 * - `filesDir/sujian/mirror/backup/<txId>/<relativePath>` — 旧正文备份
 * - `filesDir/sujian/mirror/manifest.json` — manifest 文件
 * - `filesDir/sujian/mirror/state.json` — 镜像状态（从 ReadableMirrorStateStore 迁移）
 * - `filesDir/sujian/mirror/pending-publish.json` — pending journal
 *
 * ## 与 [ReadableMirrorStorage] 的关系
 * [ReadableMirrorStorage] 的 `stageText`、`backupCommitted` 等方法把事务中间文件
 * 写到 `Download/Sujian/.staging/` 和 `.backup/`（公开目录），用户会在文件管理器
 * 看到这些内部事务目录。本类把同样的逻辑移到私有目录，`Download/Sujian/` 只保留
 * 最终用户可见的 `.md` 文件。
 *
 * ## 架构约束
 * - 位于 `:app` 的 `storage/mirror` 包，依赖 `:core:platform` 的 [AndroidPrivateDataRoot]（合法）。
 * - 不把 `content://` URI 传给 Rust——staging/backup 使用私有文件路径。
 * - 文件操作用 try-catch 包裹，失败返回 null/false。
 *
 * @param context 应用 [Context]，用于访问 `filesDir/sujian/mirror/`。
 */
class MirrorTransactionWorkspace(
    private val context: Context,
) {
    private val mirrorDir: File get() = AndroidPrivateDataRoot.mirror(context)
    private val stagingDir: File get() = File(mirrorDir, STAGING_DIR_NAME)
    private val backupDir: File get() = File(mirrorDir, BACKUP_DIR_NAME)

    // ── 暂存（staging）──

    /**
     * 暂存正文到私有目录 `staging/<txId>/<relativePath>`。
     *
     * 写入成功后返回 [StagedMirrorRef]，其中：
     * - [StagedMirrorRef.stagingUri] 是私有文件路径（如
     *   `/data/user/0/com.xiwei.sujian/files/sujian/mirror/staging/<txId>/作品/作品名/卷名/章节名.md`）
     * - [StagedMirrorRef.stagingRelativePath] 是 `staging/<txId>/<relativePath>`
     *
     * @param txId 事务 ID
     * @param relativePath 相对 `Download/Sujian/` 的目标路径（如 `作品/作品名/卷名/章节名.md`）
     * @param mimeType MIME 类型（如 `text/markdown`）
     * @param text 正文内容
     * @return 暂存引用；失败返回 null
     */
    fun stageText(
        txId: String,
        relativePath: String,
        mimeType: String,
        text: String,
    ): StagedMirrorRef? {
        return try {
            val stagingTxDir = File(stagingDir, txId)
            val targetFile = File(stagingTxDir, relativePath)
            targetFile.parentFile?.mkdirs()
            targetFile.writeText(text, Charsets.UTF_8)
            StagedMirrorRef(
                txId = txId,
                stagingUri = targetFile.absolutePath,
                stagingRelativePath = "$STAGING_DIR_NAME/$txId/$relativePath",
                finalRelativePath = relativePath,
                mimeType = mimeType,
            )
        } catch (_: IOException) {
            null
        } catch (_: Exception) {
            null
        }
    }

    /**
     * 从私有暂存读取内容。
     *
     * @param stagedRef 暂存引用（[stageText] 返回的 [StagedMirrorRef]）
     * @return 正文内容；读取失败返回 null
     */
    fun readStaged(stagedRef: StagedMirrorRef): String? {
        return try {
            val file = File(stagedRef.stagingUri)
            if (file.exists()) file.readText(Charsets.UTF_8) else null
        } catch (_: IOException) {
            null
        } catch (_: Exception) {
            null
        }
    }

    /**
     * 删除私有暂存文件。
     *
     * @param stagedRef 暂存引用
     * @return true 表示删除成功或文件本就不存在；false 表示删除失败
     */
    fun deleteStaged(stagedRef: StagedMirrorRef): Boolean {
        return try {
            val file = File(stagedRef.stagingUri)
            !file.exists() || file.delete()
        } catch (_: Exception) {
            false
        }
    }

    // ── 备份（backup）──

    /**
     * 将旧正文内容备份到私有目录 `backup/<txId>/<relativePath>`。
     *
     * @param txId 事务 ID
     * @param oldRef 旧正文引用（包含旧正文内容和相对路径）
     * @param content 旧正文内容（由调用方从 [oldRef] 读取后传入）
     * @return backup 引用（[MirrorFileRef] 的 uri 是私有文件路径）；失败返回 null
     */
    fun prepareBackup(
        txId: String,
        oldRef: MirrorFileRef,
        content: String,
    ): MirrorFileRef? {
        return try {
            val backupTxDir = File(backupDir, txId)
            val backupFile = File(backupTxDir, oldRef.relativePath)
            backupFile.parentFile?.mkdirs()
            backupFile.writeText(content, Charsets.UTF_8)
            val backupRelativePath = "$BACKUP_DIR_NAME/$txId/${oldRef.relativePath}"
            MirrorFileRef(
                uri = backupFile.absolutePath,
                relativePath = backupRelativePath,
            )
        } catch (_: IOException) {
            null
        } catch (_: Exception) {
            null
        }
    }

    /**
     * 从私有备份读取内容。
     *
     * @param backupRef backup 引用（[prepareBackup] 返回的 [MirrorFileRef]）
     * @return 正文内容；读取失败返回 null
     */
    fun readBackup(backupRef: MirrorFileRef): String? {
        return try {
            val file = File(backupRef.uri)
            if (file.exists()) file.readText(Charsets.UTF_8) else null
        } catch (_: IOException) {
            null
        } catch (_: Exception) {
            null
        }
    }

    /**
     * 删除私有备份文件。
     *
     * @param backupRef backup 引用
     * @return true 表示删除成功或文件本就不存在；false 表示删除失败
     */
    fun deleteBackup(backupRef: MirrorFileRef): Boolean {
        return try {
            val file = File(backupRef.uri)
            !file.exists() || file.delete()
        } catch (_: Exception) {
            false
        }
    }

    /**
     * 查找私有备份目录中的文件（三态结果）。
     *
     * @param txId 事务 ID
     * @param relativePath 相对 `Download/Sujian/` 的路径（与 backup 中的相对路径一致）
     * @return [MirrorLookupResult.Found] / [MirrorLookupResult.Missing] / [MirrorLookupResult.Failed]
     */
    fun lookupBackup(
        txId: String,
        relativePath: String,
    ): MirrorLookupResult {
        return try {
            val backupFile = File(File(backupDir, txId), relativePath)
            if (backupFile.exists()) {
                val backupRelativePath = "$BACKUP_DIR_NAME/$txId/$relativePath"
                MirrorLookupResult.Found(
                    MirrorFileRef(
                        uri = backupFile.absolutePath,
                        relativePath = backupRelativePath,
                    ),
                )
            } else {
                MirrorLookupResult.Missing
            }
        } catch (_: Exception) {
            MirrorLookupResult.Failed()
        }
    }

    // ── 事务回滚 ──

    /**
     * 删除 `staging/<txId>/` 和 `backup/<txId>/` 整个目录。
     *
     * 事务回滚时调用，清理该事务的所有中间状态文件。
     *
     * @param txId 事务 ID
     * @return true 表示清理成功（目录不存在也算成功）；false表示清理失败
     */
    fun rollback(txId: String): Boolean {
        return try {
            val txStagingDir = File(stagingDir, txId)
            val txBackupDir = File(backupDir, txId)
            val stagingDeleted = !txStagingDir.exists() || txStagingDir.deleteRecursively()
            val backupDeleted = !txBackupDir.exists() || txBackupDir.deleteRecursively()
            stagingDeleted && backupDeleted
        } catch (_: Exception) {
            false
        }
    }

    // ── Manifest ──

    /**
     * 将 manifest 写到 `mirror/manifest.json`（原子写入）。
     *
     * 使用 [AtomicFile] 确保写入原子性：要么完整写入，要么不影响旧文件。
     *
     * @param json manifest JSON 字符串
     * @return true 表示写入成功；false 表示写入失败
     */
    fun writeManifest(json: String): Boolean {
        return try {
            mirrorDir.mkdirs()
            val atomicFile = AtomicFile(manifestFile())
            val os = atomicFile.startWrite() as java.io.FileOutputStream
            try {
                os.write(json.toByteArray(Charsets.UTF_8))
                atomicFile.finishWrite(os)
                true
            } catch (e: IOException) {
                atomicFile.failWrite(os)
                false
            }
        } catch (_: IOException) {
            false
        } catch (_: Exception) {
            false
        }
    }

    /**
     * 从私有目录读取 manifest。
     *
     * @return manifest JSON 字符串；文件不存在或读取失败返回 null
     */
    fun readManifest(): String? {
        return try {
            val file = manifestFile()
            if (file.exists()) file.readText(Charsets.UTF_8) else null
        } catch (_: IOException) {
            null
        } catch (_: Exception) {
            null
        }
    }

    /**
     * 删除私有 manifest。
     *
     * @return true 表示删除成功或文件本就不存在；false 表示删除失败
     */
    fun deleteManifest(): Boolean {
        return try {
            val file = manifestFile()
            !file.exists() || file.delete()
        } catch (_: Exception) {
            false
        }
    }

    /**
     * 返回 manifest 文件路径（供 stateStore 记录）。
     *
     * @return `filesDir/sujian/mirror/manifest.json` 的 [File] 对象
     */
    fun manifestFile(): File = File(mirrorDir, MANIFEST_FILE_NAME)

    companion object {
        private const val STAGING_DIR_NAME = "staging"
        private const val BACKUP_DIR_NAME = "backup"
        private const val MANIFEST_FILE_NAME = "manifest.json"
    }
}
