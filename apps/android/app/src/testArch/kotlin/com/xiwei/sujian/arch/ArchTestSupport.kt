package com.xiwei.sujian.arch

import java.io.File

/**
 * 架构约束测试共享工具。
 *
 * 通过扫描源文件的 import 语句与全限定名引用来验证分层规则。
 * 检测逻辑同时识别 `import xxx.Yyy` 与正文中的 `xxx.Yyy` 全限定名引用，
 * 因此无论是 import 还是直接全限定名使用都能被抓到。
 */
internal object ArchTestSupport {
    /** app 模块主源码根目录（com/xiwei/sujian 包）。 */
    val appSourceRoot: File = locateAppSourceRoot()

    /** core-designsystem 模块主源码根目录。 */
    val designSystemSourceRoot: File = locateDesignSystemSourceRoot()

    /** core-designsystem 模块根目录（用于读取 build.gradle.kts）。 */
    val designSystemModuleRoot: File = locateModuleRoot(designSystemSourceRoot, "core-designsystem")

    /** app 模块根目录（用于读取 build.gradle.kts）。 */
    val appModuleRoot: File = locateModuleRoot(appSourceRoot, "app")

    /**
     * 收集指定目录下所有 Kotlin 源文件（.kt），排除 build/generated 产物。
     *
     * @param root 起始目录
     * @param pathFilter 路径包含片段过滤，仅保留路径包含该片段的文件
     */
    fun collectKotlinFiles(
        root: File,
        pathFilter: String? = null,
    ): List<File> {
        if (!root.exists()) return emptyList()
        return root.walkTopDown()
            .filter { it.isFile && it.extension == "kt" }
            .filter { !it.path.contains("/build/") && !it.path.contains("/generated/") }
            .filter { pathFilter == null || it.path.contains(pathFilter) }
            .toList()
    }

    /**
     * 判断文件内容是否包含指定全限定名引用。
     *
     * 同时匹配 `import xxx.Yyy` 与正文中的 `xxx.Yyy` 全限定名引用。
     * 忽略单行注释（// ...）中的引用，避免注释误报。
     */
    fun referencesFullyQualified(
        file: File,
        fqn: String,
    ): Boolean {
        if (!file.exists()) return false
        val content = file.readText()
        return content.lineSequence()
            .map { stripLineComment(it) }
            .any { it.contains(fqn) }
    }

    /**
     * 判断文件内容是否包含任一指定全限定名引用。
     */
    fun referencesAny(
        file: File,
        fqns: List<String>,
    ): List<String> {
        return fqns.filter { referencesFullyQualified(file, it) }
    }

    /**
     * 扫描指定目录下所有 Kotlin 文件，返回引用了任一禁止全限定名的违规文件。
     *
     * @return 违规文件到其命中的禁止引用列表的映射
     */
    fun findViolations(
        root: File,
        pathFilter: String?,
        forbiddenReferences: List<String>,
    ): Map<File, List<String>> {
        return collectKotlinFiles(root, pathFilter)
            .mapNotNull { file ->
                val hits = referencesAny(file, forbiddenReferences)
                if (hits.isEmpty()) null else file to hits
            }
            .toMap()
    }

    /**
     * 扫描指定文件列表，返回引用了任一禁止全限定名的违规文件。
     */
    fun findViolationsIn(
        files: List<File>,
        forbiddenReferences: List<String>,
    ): Map<File, List<String>> {
        return files.mapNotNull { file ->
            val hits = referencesAny(file, forbiddenReferences)
            if (hits.isEmpty()) null else file to hits
        }.toMap()
    }

    /**
     * 检查文件中是否存在引用了 [prefix] 但不属于任何 [allowedFqns] 的行。
     *
     * 典型场景：检查文件是否引用了 `uniffi.writer_core` 前缀但只允许
     * `uniffi.writer_core.EditorTransactionCauseDto` 等特定类型。
     * 逐行检查：如果某行包含 [prefix] 但不包含任何 [allowedFqns] 中的全限定名，
     * 则该行构成违规。
     *
     * @return 违规行号到行内容的映射，空表示无违规
     */
    fun findForbiddenPrefixRefs(
        file: File,
        prefix: String,
        allowedFqns: List<String>,
    ): Map<Int, String> {
        if (!file.exists()) return emptyMap()
        val violations = mutableMapOf<Int, String>()
        file.readText().lineSequence().forEachIndexed { index, rawLine ->
            val line = stripLineComment(rawLine)
            if (line.contains(prefix) && allowedFqns.none { line.contains(it) }) {
                violations[index + 1] = rawLine.trim()
            }
        }
        return violations
    }

    /**
     * 读取文件所有 import 语句中的全限定名（去掉 import 前缀）。
     */
    fun importsOf(file: File): List<String> {
        if (!file.exists()) return emptyList()
        return file.readText().lineSequence()
            .map { it.trim() }
            .filter { it.startsWith("import ") }
            .map { it.removePrefix("import ").trim().substringBefore(" as ").trim() }
            .toList()
    }

    /**
     * 格式化违规报告为可读字符串。
     */
    fun formatViolations(violations: Map<File, List<String>>): String {
        if (violations.isEmpty()) return ""
        return violations.entries.joinToString("\n") { (file, hits) ->
            "  - ${file.path} 命中: ${hits.joinToString(", ")}"
        }
    }

    private fun stripLineComment(line: String): String {
        val idx = line.indexOf("//")
        return if (idx >= 0) line.substring(0, idx) else line
    }

    private fun locateAppSourceRoot(): File {
        // 单元测试工作目录通常是 app 模块根目录。
        val candidates =
            listOf(
                File("src/main/kotlin/com/xiwei/sujian"),
                File("app/src/main/kotlin/com/xiwei/sujian"),
                File("apps/android/app/src/main/kotlin/com/xiwei/sujian"),
            )
        return candidates.firstOrNull { it.exists() && it.isDirectory }
            ?: error(
                "无法定位 app 主源码根目录。尝试的路径: " +
                    candidates.joinToString(", ") { it.path },
            )
    }

    private fun locateDesignSystemSourceRoot(): File {
        val candidates =
            listOf(
                File("../core-designsystem/src/main/kotlin/com/xiwei/sujian/designsystem"),
                File("core-designsystem/src/main/kotlin/com/xiwei/sujian/designsystem"),
                File("apps/android/core-designsystem/src/main/kotlin/com/xiwei/sujian/designsystem"),
            )
        return candidates.firstOrNull { it.exists() && it.isDirectory }
            ?: error(
                "无法定位 core-designsystem 主源码根目录。尝试的路径: " +
                    candidates.joinToString(", ") { it.path },
            )
    }

    /**
     * 从源码根目录向上查找包含 build.gradle.kts 的模块根目录。
     * 相对路径的 parentFile 链在到达空路径后会返回 null，因此用绝对路径向上遍历更稳健。
     */
    private fun locateModuleRoot(
        sourceRoot: File,
        moduleName: String,
    ): File {
        var current: File? = sourceRoot.absoluteFile
        var depth = 0
        while (current != null && depth < 12) {
            val buildScript = File(current, "build.gradle.kts")
            if (buildScript.exists()) {
                return current
            }
            current = current.parentFile
            depth++
        }
        error("无法定位 $moduleName 模块根目录（含 build.gradle.kts），从 ${sourceRoot.path} 向上查找失败")
    }
}
