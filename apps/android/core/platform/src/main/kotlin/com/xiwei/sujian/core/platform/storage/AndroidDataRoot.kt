package com.xiwei.sujian.core.platform.storage

import android.os.Environment
import java.io.File

/**
 * AndroidDataRoot — 平台数据根目录的唯一事实来源。
 *
 * 所有业务数据统一存放在共享存储下的 `Sujian/` 目录（稳定 ASCII 磁盘契约，
 * 不随界面语言变化，见 Issue #609 二）：
 * - `Sujian/projects/`  — 项目（作品）数据
 * - `Sujian/logs/`      — 诊断日志与 crash 记录
 * - `Sujian/exports/`   — 导出产物
 * - `Sujian/backups/`   — 备份
 *
 * 界面显示名（“素笺 / 作品 / 日志 / 导出 / 备份”）一律走 `strings.xml`，
 * 禁止从真实目录名反推显示文字。
 *
 * ## 架构定位
 * - 全局单例，只负责目录路径获取与创建，不实现任何业务规则。
 * - 业务规则（保存、同步、格式）全部由 Rust Core 负责。
 *
 * ## 权限前提
 * - Android 11+ 需要持有 `MANAGE_EXTERNAL_STORAGE` 权限（见 [hasStorageAccess]）。
 * - 调用方必须在 [hasStorageAccess] 返回 true 后才能调用 [ensureDirectories]。
 */
object AndroidDataRoot {
    /** 新数据根目录名：稳定 ASCII，长期磁盘路径契约（#609 二）。 */
    private const val ROOT_DIR_NAME = "Sujian"
    private const val PROJECTS_DIR_NAME = "projects"
    private const val LOGS_DIR_NAME = "logs"
    private const val EXPORTS_DIR_NAME = "exports"
    private const val BACKUPS_DIR_NAME = "backups"

    /**
     * 旧版中文目录名（#609 二 迁移源）：早期版本把界面中文显示名
     * 直接用作磁盘路径，仅在迁移旧数据时使用，不是新数据的路径契约。
     */
    private const val LEGACY_ROOT_DIR_NAME = "素笺"
    private const val LEGACY_PROJECTS_DIR_NAME = "作品"
    private const val LEGACY_LOGS_DIR_NAME = "日志"
    private const val LEGACY_EXPORTS_DIR_NAME = "导出"
    private const val LEGACY_BACKUPS_DIR_NAME = "备份"

    /** 共享存储根目录下的 `Sujian/`。 */
    fun rootDir(): File = File(Environment.getExternalStorageDirectory(), ROOT_DIR_NAME)

    /** `Sujian/projects/` — 项目数据根目录，对应 Core 的 `projects_root`。 */
    fun projectsDir(): File = File(rootDir(), PROJECTS_DIR_NAME)

    /** `Sujian/logs/` — 诊断日志与 crash 记录。 */
    fun logsDir(): File = File(rootDir(), LOGS_DIR_NAME)

    /** `Sujian/exports/` — 导出产物。 */
    fun exportsDir(): File = File(rootDir(), EXPORTS_DIR_NAME)

    /** `Sujian/backups/` — 备份。 */
    fun backupsDir(): File = File(rootDir(), BACKUPS_DIR_NAME)

    /** 创建所有业务子目录（幂等）。调用前需确保已持有存储访问权限。 */
    fun ensureDirectories() {
        rootDir().mkdirs()
        projectsDir().mkdirs()
        logsDir().mkdirs()
        exportsDir().mkdirs()
        backupsDir().mkdirs()
    }

    /**
     * 是否拥有共享存储访问权限。
     *
     * 基线为 API 30（minSdk=30）：共享存储的普通文件路径语义只由
     * [Environment.isExternalStorageManager] 保证，低版本不再伪装兼容
     * （旧存储权限无法在 targetSdk=36 下提供 `/storage/emulated/0` 的
     * 普通文件路径语义，见 Issue #600）。
     */
    fun hasStorageAccess(): Boolean = Environment.isExternalStorageManager()

    /**
     * 一次性迁移旧版中文数据目录（#609 二）。
     *
     * 早期版本使用 `/素笺/作品|日志|导出|备份` 作为磁盘路径契约；本方法在
     * 启动取得存储权限后、初始化 Core 前调用，把旧目录中的现有内容移动到
     * 新的 `/Sujian/` 对应目录：
     * - 文件按相对路径逐项移动，目标已存在时跳过（不覆盖已有新数据）；
     * - 旧目录下的未知子目录/文件整体并入 `Sujian/` 对应位置；
     * - 迁移完成后只删除空的旧目录，绝不删除任何文件。
     *
     * 幂等：首次执行后旧目录已空并被删除，再次调用为空操作。
     * 调用方需确保已持有存储访问权限。
     */
    fun migrateLegacyChineseDataRoot() {
        val legacyRoot = File(Environment.getExternalStorageDirectory(), LEGACY_ROOT_DIR_NAME)
        if (!legacyRoot.isDirectory) return

        val legacyToNewDirs =
            mapOf(
                LEGACY_PROJECTS_DIR_NAME to projectsDir(),
                LEGACY_LOGS_DIR_NAME to logsDir(),
                LEGACY_EXPORTS_DIR_NAME to exportsDir(),
                LEGACY_BACKUPS_DIR_NAME to backupsDir(),
            )
        val knownLegacyNames = legacyToNewDirs.keys

        // 1. 已知业务子目录：整树并入对应的新目录（文件级合并，不覆盖已有文件）。
        legacyToNewDirs.forEach { (legacyName, newDir) ->
            val legacyDir = File(legacyRoot, legacyName)
            if (legacyDir.isDirectory) {
                mergeTree(legacyDir, newDir)
            }
        }

        // 2. 旧根下未知的子目录/散落文件：并入新根（同样按文件级合并，
        //    不覆盖已有文件；已知业务子目录已在第 1 步处理，不得以中文名重建）。
        legacyRoot.listFiles()?.forEach { child ->
            if (child.name in knownLegacyNames) return@forEach
            val target = File(rootDir(), child.name)
            if (child.isDirectory) {
                mergeTree(child, target)
            } else if (child.isFile && !target.exists()) {
                target.parentFile?.mkdirs()
                child.renameTo(target)
            }
        }

        // 3. 删除迁移后遗留的空目录（自底向上，仅空目录）。
        deleteEmptyDirsBottomUp(legacyRoot)
    }

    /**
     * 把 [source] 目录树中的每个文件移动到 [targetRoot] 的对应相对路径下；
     * 目标文件已存在时跳过，绝不覆盖。目录本身不移动，仅创建目标父目录。
     */
    private fun mergeTree(
        source: File,
        targetRoot: File,
    ) {
        if (!source.isDirectory) return
        source.listFiles()?.forEach { child ->
            val target = File(targetRoot, child.name)
            if (child.isDirectory) {
                mergeTree(child, target)
            } else if (child.isFile && !target.exists()) {
                target.parentFile?.mkdirs()
                child.renameTo(target)
            }
        }
    }

    /** 自底向上删除空目录（含旧根自身）。非空目录保留，不删除任何文件。 */
    private fun deleteEmptyDirsBottomUp(dir: File) {
        if (!dir.exists() || !dir.isDirectory) return
        dir.listFiles()?.forEach { child ->
            if (child.isDirectory) {
                deleteEmptyDirsBottomUp(child)
            }
        }
        dir.delete()
    }
}
