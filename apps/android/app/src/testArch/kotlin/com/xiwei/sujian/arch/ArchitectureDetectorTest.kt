package com.xiwei.sujian.arch

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * 检测逻辑有效性验证测试。
 *
 * 通过构造临时违规源文件，验证 [ArchTestSupport] 的检测函数确实能抓到真实违规样例，
 * 确保架构约束测试不是空转——当源码出现违规时检测器必然报警。
 *
 * 这些测试本身不 @Ignore，必须始终通过，以保证检测逻辑可靠。
 */
class ArchitectureDetectorTest {
    @Test
    fun `detector flags import statement referencing forbidden fqn`() {
        val tempFile =
            createTempKotlinFile(
                """
                package com.example.violation

                import uniffi.writer_core.FooDto

                class Violation {
                    fun use(): FooDto = FooDto()
                }
                """.trimIndent(),
            )
        try {
            val hits = ArchTestSupport.referencesAny(tempFile, listOf("uniffi.writer_core"))
            assertTrue("检测器应识别 import 语句中的 uniffi.writer_core 引用", hits.isNotEmpty())
        } finally {
            tempFile.delete()
        }
    }

    @Test
    fun `detector flags fully qualified usage without import`() {
        val tempFile =
            createTempKotlinFile(
                """
                package com.example.violation

                class Violation {
                    fun use(): uniffi.writer_core.FooDto = uniffi.writer_core.FooDto()
                }
                """.trimIndent(),
            )
        try {
            val hits = ArchTestSupport.referencesAny(tempFile, listOf("uniffi.writer_core"))
            assertTrue("检测器应识别全限定名 uniffi.writer_core 引用", hits.isNotEmpty())
        } finally {
            tempFile.delete()
        }
    }

    @Test
    fun `detector ignores references inside line comments`() {
        val tempFile =
            createTempKotlinFile(
                """
                package com.example.clean

                // import uniffi.writer_core.FooDto  -- 不应误报
                class Clean {
                    // uniffi.writer_core 也不应误报
                }
                """.trimIndent(),
            )
        try {
            val hits = ArchTestSupport.referencesAny(tempFile, listOf("uniffi.writer_core"))
            assertTrue("注释中的引用不应被误报为违规", hits.isEmpty())
        } finally {
            tempFile.delete()
        }
    }

    @Test
    fun `detector flags compose dependency in data layer`() {
        val tempFile =
            createTempKotlinFile(
                """
                package com.xiwei.sujian.data

                import androidx.compose.runtime.Composable

                class BadBridge {
                    @Composable fun render() {}
                }
                """.trimIndent(),
            )
        try {
            val hits = ArchTestSupport.referencesAny(tempFile, listOf("androidx.compose"))
            assertTrue("检测器应识别 data 层对 androidx.compose 的引用", hits.isNotEmpty())
        } finally {
            tempFile.delete()
        }
    }

    @Test
    fun `detector flags jna import in ui layer`() {
        val tempFile =
            createTempKotlinFile(
                """
                package com.xiwei.sujian.ui

                import com.sun.jna.Library

                class BadUi(val lib: Library)
                """.trimIndent(),
            )
        try {
            val hits = ArchTestSupport.referencesAny(tempFile, listOf("com.sun.jna"))
            assertTrue("检测器应识别 UI 层对 com.sun.jna 的引用", hits.isNotEmpty())
        } finally {
            tempFile.delete()
        }
    }

    @Test
    fun `detector does not flag clean file`() {
        val tempFile =
            createTempKotlinFile(
                """
                package com.xiwei.sujian.ui

                import androidx.compose.runtime.Composable
                import com.xiwei.sujian.data.SettingsRepository

                class CleanViewModel(val repo: SettingsRepository)
                """.trimIndent(),
            )
        try {
            val hits =
                ArchTestSupport.referencesAny(
                    tempFile,
                    listOf("uniffi.writer_core", "com.sun.jna"),
                )
            assertTrue("干净文件不应被误报", hits.isEmpty())
        } finally {
            tempFile.delete()
        }
    }

    @Test
    fun `findViolationsIn aggregates multiple violating files`() {
        val file1 =
            createTempKotlinFile(
                """
                import uniffi.writer_core.A
                """.trimIndent(),
            )
        val file2 =
            createTempKotlinFile(
                """
                import uniffi.writer_core.B
                """.trimIndent(),
            )
        val clean =
            createTempKotlinFile(
                """
                class Clean
                """.trimIndent(),
            )
        try {
            val violations =
                ArchTestSupport.findViolationsIn(
                    listOf(file1, file2, clean),
                    listOf("uniffi.writer_core"),
                )
            assertEquals("应识别出 2 个违规文件", 2, violations.size)
            assertFalse("干净文件不应出现在违规集合中", violations.containsKey(clean))
        } finally {
            file1.delete()
            file2.delete()
            clean.delete()
        }
    }

    @Test
    fun `importsOf extracts import fully qualified names`() {
        val tempFile =
            createTempKotlinFile(
                """
                package com.example

                import uniffi.writer_core.A
                import androidx.compose.runtime.B as ComposeB
                import java.io.File

                class X
                """.trimIndent(),
            )
        try {
            val imports = ArchTestSupport.importsOf(tempFile)
            assertTrue("应包含 uniffi.writer_core.A", imports.contains("uniffi.writer_core.A"))
            assertTrue("应包含 androidx.compose.runtime.B", imports.contains("androidx.compose.runtime.B"))
            assertTrue("应包含 java.io.File", imports.contains("java.io.File"))
        } finally {
            tempFile.delete()
        }
    }

    @Test
    fun `app source root is located and contains expected packages`() {
        val root = ArchTestSupport.appSourceRoot
        assertTrue("app 源码根目录应存在: ${root.path}", root.exists())
        assertTrue("app 源码根目录应包含 ui/ 目录", File(root, "ui").exists())
        assertTrue("app 源码根目录应包含 data/ 目录", File(root, "data").exists())
        assertTrue("app 源码根目录应包含 editor/ 目录", File(root, "editor").exists())
    }

    @Test
    fun `designsystem source root is located and contains expected packages`() {
        val root = ArchTestSupport.designSystemSourceRoot
        assertTrue("designsystem 源码根目录应存在: ${root.path}", root.exists())
    }

    private fun createTempKotlinFile(content: String): File {
        val file = File.createTempFile("arch-detector-test-", ".kt")
        file.writeText(content)
        return file
    }
}
