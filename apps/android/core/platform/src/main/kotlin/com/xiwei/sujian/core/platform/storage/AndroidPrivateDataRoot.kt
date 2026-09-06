package com.xiwei.sujian.core.platform.storage

import android.content.Context
import java.io.File

/**
 * AndroidPrivateDataRoot — 应用私有数据根目录的唯一事实来源。
 *
 * 所有业务数据统一存放在应用私有存储 `context.filesDir/sujian/` 下（小写 `sujian`，
 * 与 [com.xiwei.sujian.storage.recovery.LegacySharedStorageImporter] 写入的最终目录
 * 完全一致）。Android 私有文件系统区分大小写，根名必须全仓库统一，否则迁移器写入
 * `filesDir/sujian/` 后 Core 读取 `filesDir/Sujian/` 会找不到数据（Issue #649
 * 评论 5560685734 要求 2）。
 *
 * 目录布局：
 * - `sujian/projects/` — 项目（作品）数据
 * - `sujian/logs/`     — 诊断日志与 crash 记录
 * - `sujian/exports/`  — 导出产物
 *
 * ## 应用私有存储（#649 评论 5559763924）
 *
 * 数据根目录位于应用私有 `filesDir`，不依赖共享外部存储、不依赖运行时权限、
 * 不依赖迁移/镜像。卸载即清除，提供 POSIX 文件语义。
 * 旧版共享存储数据（`/Sujian/`、`/素笺/`）的一次性迁移改由
 * [com.xiwei.sujian.storage.recovery.LegacySharedStorageImporter] 通过 SAF
 *（`OpenDocumentTree`）在用户主动选择源目录后完成，不在启动时自动扫描全盘。
 *
 * 界面显示名（"素笺 / 作品 / 日志 / 导出"）一律走 `strings.xml`，
 * 禁止从真实目录名反推显示文字。
 *
 * ## 架构定位
 * - 全局单例，只负责目录路径获取与创建，不实现任何业务规则。
 * - 业务规则（保存、同步、格式）全部由 Rust Core 负责。
 * - 位于 `:core:platform`，不依赖 Compose、UniFFI、`:app`。
 */
object AndroidPrivateDataRoot {
    /** 新数据根目录名：小写 `sujian`，与迁移器写入的最终目录完全一致。 */
    private const val ROOT = "sujian"
    private const val PROJECTS_DIR_NAME = "projects"
    private const val LOGS_DIR_NAME = "logs"
    private const val EXPORTS_DIR_NAME = "exports"

    /**
     * 应用私有根目录 `context.filesDir/sujian`。
     *
     * 调用方需传入 [Context]；内部使用 [Context.getFilesDir]，不访问共享外部存储。
     */
    fun root(context: Context): File = File(context.filesDir, ROOT)

    /** `sujian/projects/` — 项目数据根目录，对应 Core 的 `projects_root`。 */
    fun projects(context: Context): File = File(root(context), PROJECTS_DIR_NAME)

    /** `sujian/logs/` — 诊断日志与 crash 记录。 */
    fun logs(context: Context): File = File(root(context), LOGS_DIR_NAME)

    /** `sujian/exports/` — 导出产物。 */
    fun exports(context: Context): File = File(root(context), EXPORTS_DIR_NAME)

    /**
     * 创建所有业务子目录（幂等）。应用私有存储不需要额外权限。
     *
     * 注意：迁移流程中**不能**在迁移前调用本方法，否则
     * [com.xiwei.sujian.storage.recovery.LegacySharedStorageImporter] 的
     * `renameToFinal` 会因目标目录已存在而失败。调用方必须先通过
     * [com.xiwei.sujian.storage.recovery.LegacyStorageMigrationGate] 判断旧结构
     * 是否存在，仅在不需要迁移或迁移成功后才调用本方法。
     */
    fun ensure(context: Context) {
        root(context).mkdirs()
        projects(context).mkdirs()
        logs(context).mkdirs()
        exports(context).mkdirs()
    }
}
