package com.xiwei.sujian.arch

import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * core-designsystem 模块独立性约束测试。
 *
 * 规则 6：core-designsystem 不依赖 app 模块。
 * designsystem 是底层设计系统，被 app 依赖，不能反向依赖 app。
 */
class DesignSystemIndependenceTest {
    private val designSystemRoot = ArchTestSupport.designSystemSourceRoot
    private val designSystemModuleRoot = ArchTestSupport.designSystemModuleRoot

    /**
     * core-designsystem 源码不应引用 app 模块的任何包（com.xiwei.sujian.app / com.xiwei.sujian.ui /
     * com.xiwei.sujian.data / com.xiwei.sujian.editor 等）。
     */
    @Test
    fun `designsystem does not reference app module packages`() {
        val forbiddenAppPackages =
            listOf(
                "com.xiwei.sujian.ui",
                "com.xiwei.sujian.data",
                "com.xiwei.sujian.editor",
                "com.xiwei.sujian.workspace",
                "com.xiwei.sujian.runtime",
                "com.xiwei.sujian.platform",
                "com.xiwei.sujian.model",
                "com.xiwei.sujian.labs",
                "com.xiwei.sujian.diagnostics",
                "com.xiwei.sujian.settings",
                "com.xiwei.sujian.support",
            )
        val violations =
            ArchTestSupport.findViolations(
                designSystemRoot,
                pathFilter = null,
                forbiddenReferences = forbiddenAppPackages,
            )
        assertTrue(
            "core-designsystem 不应引用 app 模块包。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty(),
        )
    }

    /**
     * core-designsystem 的 build.gradle.kts 不应声明对 :app 项目的依赖。
     */
    @Test
    fun `designsystem build script does not depend on app project`() {
        val buildScript = File(designSystemModuleRoot, "build.gradle.kts")
        assertTrue(
            "core-designsystem build.gradle.kts 应存在于 ${buildScript.path}",
            buildScript.exists(),
        )
        val content = buildScript.readText()
        // 排除注释行后检查是否声明了对 :app 的项目依赖。
        val effective =
            content.lineSequence()
                .map { line ->
                    val idx = line.indexOf("//")
                    if (idx >= 0) line.substring(0, idx) else line
                }
                .joinToString("\n")
        val dependsOnApp =
            effective.contains("project(\":app\")") ||
                effective.contains("project(\"app\")") ||
                effective.contains(":app")
        assertTrue(
            "core-designsystem 的 build.gradle.kts 不应声明对 :app 项目的依赖。",
            !dependsOnApp,
        )
    }

    /**
     * core-designsystem 源码不应引用 UniFFI / JNA / Bridge 基础设施。
     * designsystem 是纯 UI 组件库，不应触碰业务绑定层。
     */
    @Test
    fun `designsystem does not reference uniffi jna or bridge`() {
        val forbidden =
            listOf(
                "uniffi.writer_core",
                "com.sun.jna",
                "com.xiwei.sujian.data.Bridge",
            )
        val violations =
            ArchTestSupport.findViolations(
                designSystemRoot,
                pathFilter = null,
                forbiddenReferences = forbidden,
            )
        assertTrue(
            "core-designsystem 不应引用 UniFFI/JNA/Bridge。违规:\n${ArchTestSupport.formatViolations(violations)}",
            violations.isEmpty(),
        )
    }
}
