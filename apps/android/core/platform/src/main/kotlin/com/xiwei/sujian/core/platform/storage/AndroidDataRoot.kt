package com.xiwei.sujian.core.platform.storage

import android.os.Environment
import java.io.File

/**
 * AndroidDataRoot — 平台数据根目录的唯一事实来源。
 *
 * 所有业务数据统一存放在共享存储下的 `素笺/` 目录：
 * - `素笺/作品/`  — 项目（作品）数据
 * - `素笺/日志/`  — 诊断日志与 crash 记录
 * - `素笺/导出/`  — 导出产物
 * - `素笺/备份/`  — 备份
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
    private const val ROOT_DIR_NAME = "素笺"
    private const val PROJECTS_DIR_NAME = "作品"
    private const val LOGS_DIR_NAME = "日志"
    private const val EXPORTS_DIR_NAME = "导出"
    private const val BACKUPS_DIR_NAME = "备份"

    /** 共享存储根目录下的 `素笺/`。 */
    fun rootDir(): File = File(Environment.getExternalStorageDirectory(), ROOT_DIR_NAME)

    /** `素笺/作品/` — 项目数据根目录，对应 Core 的 `projects_root`。 */
    fun projectsDir(): File = File(rootDir(), PROJECTS_DIR_NAME)

    /** `素笺/日志/` — 诊断日志与 crash 记录。 */
    fun logsDir(): File = File(rootDir(), LOGS_DIR_NAME)

    /** `素笺/导出/` — 导出产物。 */
    fun exportsDir(): File = File(rootDir(), EXPORTS_DIR_NAME)

    /** `素笺/备份/` — 备份。 */
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
}
