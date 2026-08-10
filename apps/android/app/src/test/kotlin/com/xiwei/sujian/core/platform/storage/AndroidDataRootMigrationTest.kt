@file:Suppress("StringLiteralDuplication") // 迁移测试固件（新旧目录名/文件内容）天然重复

package com.xiwei.sujian.core.platform.storage

import android.os.Environment
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File

/**
 * #609 二：数据目录稳定 ASCII 磁盘名 + 旧版中文目录一次性迁移测试。
 *
 * 磁盘路径契约不再绑定界面中文显示名：
 * - 新目录固定为 `Sujian/projects|logs|exports|backups`；
 * - 旧 `/素笺/` 目录在启动时迁移：文件级合并进新目录，目标已存在时
 *   跳过（不覆盖新数据），迁移后只删除空目录，幂等。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class AndroidDataRootMigrationTest {
    private val external: File = Environment.getExternalStorageDirectory()
    private val legacyRoot: File = File(external, "素笺")
    private val newRoot: File = File(external, "Sujian")

    private companion object {
        const val LEGACY_PROJECTS = "作品"
        const val LEGACY_LOGS = "日志"
        const val LEGACY_EXPORTS = "导出"
        const val LEGACY_BACKUPS = "备份"
        const val NEW_PROJECTS = "projects"
    }

    @Before
    fun cleanState() {
        legacyRoot.deleteRecursively()
        newRoot.deleteRecursively()
    }

    @Test
    fun directoryNames_areStableAscii() {
        assertEquals("Sujian", AndroidDataRoot.rootDir().name)
        assertEquals("projects", AndroidDataRoot.projectsDir().name)
        assertEquals("logs", AndroidDataRoot.logsDir().name)
        assertEquals("exports", AndroidDataRoot.exportsDir().name)
        assertEquals("backups", AndroidDataRoot.backupsDir().name)

        assertEquals(File(newRoot, "projects"), AndroidDataRoot.projectsDir())
        assertEquals(File(newRoot, "logs"), AndroidDataRoot.logsDir())
        assertEquals(File(newRoot, "exports"), AndroidDataRoot.exportsDir())
        assertEquals(File(newRoot, "backups"), AndroidDataRoot.backupsDir())
    }

    @Test
    fun migration_movesLegacyBusinessDirsIntoSujian() {
        writeFile(File(legacyRoot, LEGACY_PROJECTS), "p/novel-a/project.json", "legacy-project")
        writeFile(File(legacyRoot, LEGACY_LOGS), "app.log", "legacy-log")
        writeFile(File(legacyRoot, LEGACY_EXPORTS), "out.md", "legacy-export")
        writeFile(File(legacyRoot, LEGACY_BACKUPS), "backup.zip", "legacy-backup")

        AndroidDataRoot.migrateLegacyChineseDataRoot()

        assertEquals("legacy-project", readFile(File(newRoot, "$NEW_PROJECTS/p/novel-a/project.json")))
        assertEquals("legacy-log", readFile(File(newRoot, "logs/app.log")))
        assertEquals("legacy-export", readFile(File(newRoot, "exports/out.md")))
        assertEquals("legacy-backup", readFile(File(newRoot, "backups/backup.zip")))
        // 旧中文目录整体删除（文件全部迁走后为空）
        assertFalse("旧中文根目录应已删除", legacyRoot.exists())
        // 新根下不得重建中文业务子目录
        assertFalse(File(newRoot, LEGACY_PROJECTS).exists())
        assertFalse(File(newRoot, LEGACY_LOGS).exists())
        assertFalse(File(newRoot, LEGACY_EXPORTS).exists())
        assertFalse(File(newRoot, LEGACY_BACKUPS).exists())
    }

    @Test
    fun migration_mergesFileByFile_withoutOverwritingExistingNewData() {
        // 新数据已存在同路径文件（内容更新）
        writeFile(File(newRoot, NEW_PROJECTS), "p/a.md", "NEW-DATA")
        // 旧数据同路径文件 + 无冲突文件
        writeFile(File(legacyRoot, LEGACY_PROJECTS), "p/a.md", "OLD-DATA")
        writeFile(File(legacyRoot, LEGACY_PROJECTS), "p/b.md", "OLD-B")

        AndroidDataRoot.migrateLegacyChineseDataRoot()

        // 冲突文件：新数据保留，旧文件原样留在旧目录（不覆盖、不删除）
        assertEquals("NEW-DATA", readFile(File(newRoot, "$NEW_PROJECTS/p/a.md")))
        assertEquals("OLD-DATA", readFile(File(legacyRoot, "$LEGACY_PROJECTS/p/a.md")))
        // 无冲突文件正常迁移
        assertEquals("OLD-B", readFile(File(newRoot, "$NEW_PROJECTS/p/b.md")))
        assertFalse(File(legacyRoot, "$LEGACY_PROJECTS/p/b.md").exists())
    }

    @Test
    fun migration_movesUnknownChildrenIntoNewRoot() {
        // 旧根下未知子目录与散落文件整体并入新根
        writeFile(legacyRoot, "custom/x.txt", "x")
        writeFile(legacyRoot, "settings.local.json", "s")

        AndroidDataRoot.migrateLegacyChineseDataRoot()

        assertEquals("x", readFile(File(newRoot, "custom/x.txt")))
        assertEquals("s", readFile(File(newRoot, "settings.local.json")))
        assertFalse(legacyRoot.exists())
    }

    @Test
    fun migration_isIdempotent_andKeepsNewDataIntact() {
        writeFile(File(legacyRoot, LEGACY_PROJECTS), "p/a.md", "OLD")
        writeFile(File(newRoot, NEW_PROJECTS), "p/b.md", "NEW-B")

        AndroidDataRoot.migrateLegacyChineseDataRoot()
        AndroidDataRoot.migrateLegacyChineseDataRoot()
        AndroidDataRoot.migrateLegacyChineseDataRoot()

        assertEquals("OLD", readFile(File(newRoot, "$NEW_PROJECTS/p/a.md")))
        assertEquals("NEW-B", readFile(File(newRoot, "$NEW_PROJECTS/p/b.md")))
        assertFalse(legacyRoot.exists())

        // 迁移后再写入新数据，不应被任何后续迁移影响
        writeFile(File(newRoot, NEW_PROJECTS), "p/c.md", "NEW-C")
        AndroidDataRoot.migrateLegacyChineseDataRoot()
        assertEquals("NEW-C", readFile(File(newRoot, "$NEW_PROJECTS/p/c.md")))
    }

    @Test
    fun migration_withoutLegacyDir_isNoOp() {
        assertFalse(legacyRoot.exists())
        AndroidDataRoot.migrateLegacyChineseDataRoot()
        assertFalse(legacyRoot.exists())
    }

    private fun writeFile(
        dir: File,
        relativePath: String,
        content: String,
    ) {
        val file = File(dir, relativePath)
        file.parentFile?.mkdirs()
        file.writeText(content)
    }

    private fun readFile(file: File): String? = if (file.exists()) file.readText() else null
}
