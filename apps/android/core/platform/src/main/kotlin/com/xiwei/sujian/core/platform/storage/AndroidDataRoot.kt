package com.xiwei.sujian.core.platform.storage

import android.content.Context
import java.io.File

/**
 * AndroidDataRoot — 平台数据根目录的唯一事实来源。
 *
 * 所有业务数据统一存放在应用私有存储 `context.filesDir/Sujian/` 下（稳定 ASCII
 * 磁盘契约，不随界面语言变化，见 Issue #609 二）：
 * - `Sujian/projects/`  — 项目（作品）数据
 * - `Sujian/logs/`      — 诊断日志与 crash 记录
 * - `Sujian/exports/`   — 导出产物
 * - `Sujian/backups/`   — 备份
 *
 * ## 应用私有存储（#649 评论 5559763924）
 *
 * 数据根目录位于应用私有 `filesDir`，不再使用共享外部存储。
 * 应用私有存储提供 POSIX 文件语义、不需要运行时权限、卸载即清除。
 * 旧版共享存储数据（`/Sujian/`、`/素笺/`）的一次性迁移改由
 * [com.xiwei.sujian.storage.recovery.LegacySharedStorageImporter] 通过 SAF
 *（`OpenDocumentTree`）在用户主动选择源目录后完成，不在启动时自动扫描全盘。
 *
 * Git metadata 与 worktree 一并落在应用私有 `Sujian/projects/<id>/` 下，
 * 不再单独外置 `filesDir/sujian-git/` 基目录（#644 评论 5490206957 的外置方案
 * 随私有化收回，Core 的 `GitRepoLayout` 永远用 app_data_root 内的默认 `.git`）。
 *
 * 界面显示名（”素笺 / 作品 / 日志 / 导出 / 备份”）一律走 `strings.xml`，
 * 禁止从真实目录名反推显示文字。
 *
 * ## 架构定位
 * - 全局单例，只负责目录路径获取与创建，不实现任何业务规则。
 * - 业务规则（保存、同步、格式）全部由 Rust Core 负责。
 * - 位于 `:core:platform`，不依赖 Compose、UniFFI、`:app`。
 */
object AndroidDataRoot {
    /** 新数据根目录名：稳定 ASCII，长期磁盘路径契约（#609 二）。 */
    private const val ROOT_DIR_NAME = "Sujian"
    private const val PROJECTS_DIR_NAME = "projects"
    private const val LOGS_DIR_NAME = "logs"
    private const val EXPORTS_DIR_NAME = "exports"
    private const val BACKUPS_DIR_NAME = "backups"

    /**
     * 应用私有根目录下的 `Sujian/`（`context.filesDir/Sujian`）。
     *
     * 调用方需传入 [Context]；内部使用 [Context.getFilesDir]，不访问共享外部存储。
     */
    fun rootDir(context: Context): File = File(context.filesDir, ROOT_DIR_NAME)

    /** `Sujian/projects/` — 项目数据根目录，对应 Core 的 `projects_root`。 */
    fun projectsDir(context: Context): File = File(rootDir(context), PROJECTS_DIR_NAME)

    /** `Sujian/logs/` — 诊断日志与 crash 记录。 */
    fun logsDir(context: Context): File = File(rootDir(context), LOGS_DIR_NAME)

    /** `Sujian/exports/` — 导出产物。 */
    fun exportsDir(context: Context): File = File(rootDir(context), EXPORTS_DIR_NAME)

    /** `Sujian/backups/` — 备份。 */
    fun backupsDir(context: Context): File = File(rootDir(context), BACKUPS_DIR_NAME)

    /** 创建所有业务子目录（幂等）。应用私有存储不需要额外权限。 */
    fun ensureDirectories(context: Context) {
        rootDir(context).mkdirs()
        projectsDir(context).mkdirs()
        logsDir(context).mkdirs()
        exportsDir(context).mkdirs()
        backupsDir(context).mkdirs()
    }
}
