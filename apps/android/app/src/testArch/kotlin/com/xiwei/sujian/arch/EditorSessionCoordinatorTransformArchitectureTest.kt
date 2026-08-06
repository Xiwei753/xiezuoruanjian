package com.xiwei.sujian.arch

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * #595 五：updateSessionState transform 纯函数架构约束测试 — 源码静态检查。
 *
 * MutableStateFlow.update 的 transform 在 CAS 竞争时会重新执行。
 * transform 内写外部 store（mutableMap）违反纯函数契约 — 重复执行
 * 会重复写入 store，可能基于已被前一次写入修改的 store 状态计算错误结果。
 *
 * 本测试验证所有 updateSessionState 调用的 transform 内不直接调用
 * store.put / store.update / store.remove — store 写入只在 transform 外
 * 通过 pendingRecord?.let { store.put(it) } 执行。
 *
 * 这是源码静态结构约束（transform 体不写 store），不是运行时行为测试
 * （运行时行为由 TransformPurityTest 用真实 applyLocalEdit 驱动验证
 * store 记录与 SessionState 一致）。
 */
class EditorSessionCoordinatorTransformArchitectureTest {

    private fun coordinatorSource(): String {
        val path = System.getProperty("user.dir")!! +
            "/src/main/kotlin/com/xiwei/sujian/editor/v2/coordinator/EditorSessionCoordinator.kt"
        return File(path).readText()
    }

    /**
     * 提取所有 updateSessionState { ... } transform 块内的代码。
     * 简化匹配：从 "updateSessionState {" 到对应的 "}"（同缩进级别）。
     */
    private fun extractTransformBodies(source: String): List<String> {
        val bodies = mutableListOf<String>()
        val pattern = Regex("""updateSessionState\s*\{""")
        for (match in pattern.findAll(source)) {
            val start = match.range.last + 1
            var depth = 1
            var i = start
            while (i < source.length && depth > 0) {
                when (source[i]) {
                    '{' -> depth++
                    '}' -> depth--
                }
                i++
            }
            bodies.add(source.substring(start, i - 1))
        }
        return bodies
    }

    @Test
    fun transformBodies_doNotCallStorePut() {
        val source = coordinatorSource()
        val bodies = extractTransformBodies(source)
        assertTrue("must find at least one updateSessionState transform", bodies.isNotEmpty())
        for (body in bodies) {
            assertFalse(
                "updateSessionState transform must not call store.put — write store outside transform via pendingRecord?.let { store.put(it) } (#595 五)",
                body.contains(Regex("""store\.put\(""")),
            )
        }
    }

    @Test
    fun transformBodies_doNotCallStoreUpdate() {
        val source = coordinatorSource()
        val bodies = extractTransformBodies(source)
        for (body in bodies) {
            assertFalse(
                "updateSessionState transform must not call store.update — write store outside transform (#595 五)",
                body.contains(Regex("""store\.update\(""")),
            )
        }
    }

    @Test
    fun transformBodies_doNotCallStoreRemove() {
        val source = coordinatorSource()
        val bodies = extractTransformBodies(source)
        for (body in bodies) {
            assertFalse(
                "updateSessionState transform must not call store.remove — write store outside transform (#595 五)",
                body.contains(Regex("""store\.remove\(""")),
            )
        }
    }

    @Test
    fun pendingRecordPattern_usedForStoreWrite() {
        val source = coordinatorSource()
        assertTrue(
            "store writes must use pendingRecord?.let { store.put(it) } pattern outside transform (#595 五)",
            source.contains("pendingRecord?.let { store.put(it) }"),
        )
    }
}
