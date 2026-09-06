package com.xiwei.sujian.storage.recovery

import android.content.Context
import android.net.Uri
import com.xiwei.sujian.core.platform.storage.documents.DocumentTreeReader
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.nio.file.StandardCopyOption

/**
 * 旧版共享存储迁移结果。
 */
sealed class ImportResult {
    /** 迁移成功，调用方应初始化 `WriterAppService`。 */
    object Success : ImportResult()

    /** 未发现旧版 `filesDir/sujian-git/workspace`，无需迁移。 */
    object NoLegacyGitFound : ImportResult()

    /** 复制或移动失败，已回滚。 */
    data class CopyFailed(val reason: String) : ImportResult()
}

/**
 * LegacySharedStorageImporter — 旧版本升级迁移器。
 *
 * 处理旧 `filesDir/sujian-git/workspace` 结构。迁移顺序（Issue #649 评论 5559763924）：
 * 1. 用户通过 `ActivityResultContracts.OpenDocumentTree()` 选择旧 `Sujian` 子目录
 *    （URI 由调用方传入，本类不直接注册 launcher）。
 * 2. 先把旧树完整复制到临时目录 `filesDir/sujian-import.tmp/`。
 * 3. 复制完成后把旧 `filesDir/sujian-git/workspace` 移到临时目录的 `.git/`。
 * 4. 把整个临时目录 rename 成 `filesDir/sujian/`。
 * 5. 成功后返回，由调用方初始化 `WriterAppService`。
 * 6. 删除空掉的 `filesDir/sujian-git/`。
 *
 * 临时目录是应用私有 [Context.filesDir]，清理用 [File.deleteRecursively]（安全）。
 * rename 用 [Files.move] (NIO) 保证原子性，失败回滚。
 *
 * 安全约束：严禁裸 `rm`/`rm -rf`；清理只作用于应用私有临时目录。
 */
class LegacySharedStorageImporter {
    suspend fun import(
        context: Context,
        sourceTreeUri: Uri,
        documentTreeReader: DocumentTreeReader,
    ): ImportResult =
        withContext(Dispatchers.IO) {
            doImport(context, sourceTreeUri, documentTreeReader)
        }

    private fun doImport(
        context: Context,
        sourceTreeUri: Uri,
        documentTreeReader: DocumentTreeReader,
    ): ImportResult {
        val legacyGitWorkspace = File(context.filesDir, LEGACY_GIT_WORKSPACE_PATH)
        if (!legacyGitWorkspace.exists()) return ImportResult.NoLegacyGitFound

        val tmpDir = File(context.filesDir, TMP_DIR_NAME)
        val prepared = prepareTempDir(tmpDir)
        if (prepared != null) return prepared

        // 2-3. 复制旧树到临时目录，再把旧 sujian-git/workspace 移到 .git/
        val copyOutcome = copyTreeAndMoveGit(sourceTreeUri, tmpDir, legacyGitWorkspace, documentTreeReader)
        if (copyOutcome != null) {
            tmpDir.deleteRecursively()
            return copyOutcome
        }

        // 4. rename 临时目录成 filesDir/sujian/
        val renamed = renameToFinal(tmpDir, legacyGitWorkspace, context)
        if (renamed != null) {
            tmpDir.deleteRecursively()
            return renamed
        }

        // 6. 删除空掉的 filesDir/sujian-git/
        File(context.filesDir, LEGACY_GIT_BASE_NAME).deleteRecursively()
        return ImportResult.Success
    }

    /** 准备干净的临时目录；返回 null 表示成功。 */
    private fun prepareTempDir(tmpDir: File): ImportResult.CopyFailed? {
        if (tmpDir.exists() && !tmpDir.deleteRecursively()) {
            return ImportResult.CopyFailed("Failed to clean stale temp dir: $tmpDir")
        }
        if (!tmpDir.mkdirs()) {
            return ImportResult.CopyFailed("Failed to create temp dir: $tmpDir")
        }
        return null
    }

    /**
     * 复制旧树到 [tmpDir]，再把 [legacyGitWorkspace] 原子移到 `tmpDir/.git/`。
     * 返回 null 表示成功；返回 [ImportResult.CopyFailed] 表示失败（调用方负责清理 tmpDir）。
     */
    private fun copyTreeAndMoveGit(
        sourceTreeUri: Uri,
        tmpDir: File,
        legacyGitWorkspace: File,
        reader: DocumentTreeReader,
    ): ImportResult.CopyFailed? {
        try {
            reader.copyTree(sourceTreeUri, tmpDir)
        } catch (e: IOException) {
            return ImportResult.CopyFailed("Copy tree failed: ${e.message}")
        }
        val gitTarget = File(tmpDir, GIT_DIR_NAME)
        if (gitTarget.exists()) {
            gitTarget.deleteRecursively()
        }
        try {
            Files.move(
                legacyGitWorkspace.toPath(),
                gitTarget.toPath(),
                StandardCopyOption.ATOMIC_MOVE,
            )
        } catch (e: Exception) {
            return ImportResult.CopyFailed("Move legacy git failed: ${e.message}")
        }
        return null
    }

    /** 把 [tmpDir] 原子 rename 成 `filesDir/sujian/`；返回 null 表示成功。 */
    private fun renameToFinal(
        tmpDir: File,
        legacyGitWorkspace: File,
        context: Context,
    ): ImportResult.CopyFailed? {
        val finalDir = File(context.filesDir, FINAL_DIR_NAME)
        if (finalDir.exists()) {
            rollbackGit(File(tmpDir, GIT_DIR_NAME), legacyGitWorkspace)
            return ImportResult.CopyFailed("Target dir already exists: $finalDir")
        }
        try {
            Files.move(tmpDir.toPath(), finalDir.toPath(), StandardCopyOption.ATOMIC_MOVE)
        } catch (e: Exception) {
            rollbackGit(File(tmpDir, GIT_DIR_NAME), legacyGitWorkspace)
            return ImportResult.CopyFailed("Rename to final dir failed: ${e.message}")
        }
        return null
    }

    /** 回滚：把已移到 tmpDir/.git/ 的 git metadata 移回原位。失败仅忽略（best-effort）。 */
    private fun rollbackGit(
        gitTarget: File,
        legacyGitWorkspace: File,
    ) {
        runCatching {
            Files.move(
                gitTarget.toPath(),
                legacyGitWorkspace.toPath(),
                StandardCopyOption.ATOMIC_MOVE,
            )
        }
    }

    companion object {
        private const val LEGACY_GIT_WORKSPACE_PATH = "sujian-git/workspace"
        private const val LEGACY_GIT_BASE_NAME = "sujian-git"
        private const val TMP_DIR_NAME = "sujian-import.tmp"
        private const val FINAL_DIR_NAME = "sujian"
        private const val GIT_DIR_NAME = ".git"
    }
}
