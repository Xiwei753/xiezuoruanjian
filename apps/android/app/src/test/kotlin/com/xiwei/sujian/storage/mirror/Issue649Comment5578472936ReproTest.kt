package com.xiwei.sujian.storage.mirror

import androidx.core.util.AtomicFile
import androidx.test.core.app.ApplicationProvider
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.io.File
import java.io.FileOutputStream

/**
 * #649 评论 5578472936：`readSnapshotStrict()` 仍然不是 strict 的修复回归测试。
 *
 * 验证 comment 5578472936 指出的缺陷已修复：
 * - readSnapshotStrict 不再用 optString/optJSONObject/continue 静默裁掉坏 state
 * - getAllChapterEntriesStrict 复用同一份 decodeStateRootStrict
 * - getCommittedManifestStrict 复用同一份 decodeStateRootStrict（含 projects 损坏也判 Corrupted）
 * - decodeEntryStrict 用 obj.get() + require(type) 做真正类型检查，拒绝类型 coercion
 * - saveRestoredState 在 ReadResult.Parsed 时先跑 strict decoder
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("TooManyFunctions", "StringLiteralDuplication")
class Issue649Comment5578472936ReproTest {
    private lateinit var store: ReadableMirrorStateStore

    @Before
    fun setUp() {
        val context = ApplicationProvider.getApplicationContext<android.app.Application>()
        store = ReadableMirrorStateStore(context)
    }

    // ── helper ──

    private fun writeStateJson(json: String) {
        val dir =
            File(ApplicationProvider.getApplicationContext<android.app.Application>().noBackupFilesDir, "sujian-mirror")
        dir.mkdirs()
        val file = File(dir, "state.json")
        val atomicFile = AtomicFile(file)
        val os = atomicFile.startWrite() as FileOutputStream
        try {
            os.write(json.toByteArray(Charsets.UTF_8))
            atomicFile.finishWrite(os)
        } catch (e: Exception) {
            atomicFile.failWrite(os)
            throw e
        }
    }

    private fun buildValidStateJson(): String {
        val obj =
            JSONObject().apply {
                put("backend", "media_store")
                put("committedManifestJson", "{}")
                put("committedManifestHash", computeContentHash("{}"))
            }
        return obj.toString()
    }

    // ══════════════════════════════════════════════════════════════════════
    // readSnapshotStrict — 类型 coercion 拒绝
    // ══════════════════════════════════════════════════════════════════════

    /**
     * #649 评论 5578472936 问题：backend 是数字（类型 coercion）→ 应该被 strict decoder 拒绝。
     *
     * Android `optString()` 对非字符串值返回空字符串，旧代码用 `takeIf { it.isNotEmpty() }`
     * 就把数字 backend 当成 null 回退到 MEDIA_STORE，掩盖损坏。
     */
    @Test
    fun readSnapshotStrict_rejectsNumericBackend() {
        writeStateJson("""{"backend": 42}""")
        val result = store.readSnapshotStrict()
        assertTrue("readSnapshotStrict 应返回 failure", result.isFailure)
    }

    /**
     * treeUri 是数字 → strict decoder 拒绝。
     */
    @Test
    fun readSnapshotStrict_rejectsNumericTreeUri() {
        writeStateJson("""{"backend": "media_store", "treeUri": 123}""")
        val result = store.readSnapshotStrict()
        assertTrue("readSnapshotStrict 应返回 failure", result.isFailure)
    }

    /**
     * projects 是数组 → strict decoder 拒绝。
     */
    @Test
    fun readSnapshotStrict_rejectsArrayProjects() {
        writeStateJson("""{"backend": "media_store", "projects": []}""")
        val result = store.readSnapshotStrict()
        assertTrue("readSnapshotStrict 应返回 failure", result.isFailure)
    }

    /**
     * 某个 project 值是字符串 → strict decoder 拒绝。
     */
    @Test
    fun readSnapshotStrict_rejectsStringProjectValue() {
        writeStateJson("""{"backend": "media_store", "projects": {"p1": "not-object"}}""")
        val result = store.readSnapshotStrict()
        assertTrue("readSnapshotStrict 应返回 failure", result.isFailure)
    }

    /**
     * chapter key 没有 "/" 分隔符 → strict decoder 拒绝。
     */
    @Test
    fun readSnapshotStrict_rejectsMalformedChapterKey() {
        writeStateJson(
            """
            {"backend": "media_store",
            "projects": {"p1": {"badkey": {"uri": "u", "relativePath": "p", "revision": 1, "contentHash": "h"}}}}
            """.trimIndent(),
        )
        val result = store.readSnapshotStrict()
        assertTrue("readSnapshotStrict 应返回 failure", result.isFailure)
    }

    /**
     * chapter entry 的 uri 是数字 → strict decoder 拒绝（类型 coercion）。
     *
     * Android `getString()` 对数字会做类型 coercion 返回 "42"，旧代码不检查实际类型。
     * 修复后用 `obj.get()` + `require(value is String)` 拒绝。
     */
    @Test
    fun readSnapshotStrict_rejectsNumericUri() {
        writeStateJson(
            """
            {"backend": "media_store",
            "projects": {"p1": {"v1/ch1": {"uri": 42, "relativePath": "p", "revision": 1, "contentHash": "h"}}}}
            """.trimIndent(),
        )
        val result = store.readSnapshotStrict()
        assertTrue("readSnapshotStrict 应拒绝数字 uri", result.isFailure)
    }

    /**
     * chapter entry 的 revision 是字符串 → strict decoder 拒绝。
     */
    @Test
    fun readSnapshotStrict_rejectsStringRevision() {
        writeStateJson(
            """
            {"backend": "media_store",
            "projects": {"p1": {"v1/ch1": {"uri": "u", "relativePath": "p", "revision": "abc", "contentHash": "h"}}}}
            """.trimIndent(),
        )
        val result = store.readSnapshotStrict()
        assertTrue("readSnapshotStrict 应拒绝字符串 revision", result.isFailure)
    }

    /**
     * chapter entry 的 contentHash 是数字 → strict decoder 拒绝。
     */
    @Test
    fun readSnapshotStrict_rejectsNumericContentHash() {
        writeStateJson(
            """
            {"backend": "media_store",
            "projects": {"p1": {"v1/ch1": {"uri": "u", "relativePath": "p", "revision": 1, "contentHash": 42}}}}
            """.trimIndent(),
        )
        val result = store.readSnapshotStrict()
        assertTrue("readSnapshotStrict 应拒绝数字 contentHash", result.isFailure)
    }

    /**
     * committedManifestJson 是数字 → strict decoder 拒绝。
     */
    @Test
    fun readSnapshotStrict_rejectsNumericCommittedManifestJson() {
        writeStateJson("""{"backend": "media_store", "committedManifestJson": 42, "committedManifestHash": "abc"}""")
        val result = store.readSnapshotStrict()
        assertTrue("readSnapshotStrict 应拒绝数字 committedManifestJson", result.isFailure)
    }

    /**
     * publishedProjectIds 是数组 → strict decoder 拒绝。
     */
    @Test
    fun readSnapshotStrict_rejectsArrayPublishedProjectIds() {
        writeStateJson("""{"backend": "media_store", "publishedProjectIds": ["p1"]}""")
        val result = store.readSnapshotStrict()
        assertTrue("readSnapshotStrict 应拒绝数组 publishedProjectIds", result.isFailure)
    }

    // ══════════════════════════════════════════════════════════════════════
    // readSnapshotStrict — 合法状态
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 合法完整 state → readSnapshotStrict 返回 success 且数据正确。
     */
    @Test
    fun readSnapshotStrict_validState_returnsSuccess() {
        writeStateJson(buildValidStateJson())
        val result = store.readSnapshotStrict()
        assertTrue("合法 state → success", result.isSuccess)
        val snapshot = result.getOrNull()!!
        assertEquals(MirrorBackend.MEDIA_STORE, snapshot.backend)
        assertNull(snapshot.treeUri)
        assertNull(snapshot.manifestUri)
        assertTrue(snapshot.projects.isEmpty())
    }

    /**
     * 合法 state 含 chapter entries → readSnapshotStrict 返回正确数据。
     */
    @Test
    fun readSnapshotStrict_validWithChapters_returnsChapters() {
        val json =
            """
            {
                "backend": "document_tree",
                "treeUri": "content://tree/doc",
                "manifestUri": "content://manifest",
                "projects": {
                    "proj1": {
                        "vol1/ch1": {
                            "uri": "content://media/1",
                            "relativePath": "作品/P/V/Ch1.md",
                            "revision": 100,
                            "contentHash": "sha256:abc"
                        }
                    }
                },
                "publishedProjectIds": {"proj1": true},
                "committedManifestJson": "{}",
                "committedManifestHash": "${computeContentHash("{}")}"
            }
            """.trimIndent()
        writeStateJson(json)
        val result = store.readSnapshotStrict()
        assertTrue(result.isSuccess)
        val snapshot = result.getOrNull()!!
        assertEquals(MirrorBackend.DOCUMENT_TREE, snapshot.backend)
        assertEquals("content://tree/doc", snapshot.treeUri)
        assertEquals("content://manifest", snapshot.manifestUri)
        assertEquals(1, snapshot.projects.size)
        val projEntries = snapshot.projects["proj1"]!!
        assertEquals(1, projEntries.size)
        val key = ChapterKey("proj1", "vol1", "ch1")
        assertTrue(projEntries.containsKey(key))
        assertEquals("content://media/1", projEntries[key]!!.uri)
    }

    /**
     * 首次安装（state.json 不存在）→ 返回默认 MEDIA_STORE 快照。
     */
    @Test
    fun readSnapshotStrict_notExists_returnsDefaultSnapshot() {
        // 删除 state.json（如果存在）
        val dir =
            File(ApplicationProvider.getApplicationContext<android.app.Application>().noBackupFilesDir, "sujian-mirror")
        File(dir, "state.json").delete()
        val result = store.readSnapshotStrict()
        assertTrue(result.isSuccess)
        val snapshot = result.getOrNull()!!
        assertEquals(MirrorBackend.MEDIA_STORE, snapshot.backend)
        assertNull(snapshot.treeUri)
        assertTrue(snapshot.projects.isEmpty())
    }

    // ══════════════════════════════════════════════════════════════════════
    // getAllChapterEntriesStrict — 复用 decodeStateRootStrict
    // ══════════════════════════════════════════════════════════════════════

    /**
     * projects 损坏（非 JSONObject value）→ getAllChapterEntriesStrict 返回 failure。
     *
     * 旧代码独立遍历 JSON，projects 损坏时静默跳过。
     * 修复后复用 decodeStateRootStrict，损坏整体报错。
     */
    @Test
    fun getAllChapterEntriesStrict_corruptedProjects_returnsFailure() {
        writeStateJson("""{"backend": "media_store", "projects": {"p1": "bad"}}""")
        val result = store.getAllChapterEntriesStrict()
        assertTrue("projects 损坏 → failure", result.isFailure)
    }

    /**
     * 合法 state → getAllChapterEntriesStrict 返回正确 publishedProjectIds + entries。
     */
    @Test
    fun getAllChapterEntriesStrict_validState_returnsCorrectData() {
        val json =
            """
            {
                "backend": "media_store",
                "projects": {
                    "proj1": {
                        "vol1/ch1": {"uri": "u", "relativePath": "p", "revision": 1, "contentHash": "h"}
                    }
                },
                "publishedProjectIds": {"proj1": true, "proj2": true},
                "committedManifestJson": "{}",
                "committedManifestHash": "${computeContentHash("{}")}"
            }
            """.trimIndent()
        writeStateJson(json)
        val result = store.getAllChapterEntriesStrict()
        assertTrue(result.isSuccess)
        val (ids, entries) = result.getOrNull()!!
        assertTrue(ids.contains("proj1"))
        assertTrue(ids.contains("proj2"))
        assertEquals(1, entries.size)
        assertTrue(entries.containsKey(ChapterKey("proj1", "vol1", "ch1")))
    }

    // ══════════════════════════════════════════════════════════════════════
    // getCommittedManifestStrict — 复用 decodeStateRootStrict
    // ══════════════════════════════════════════════════════════════════════

    /**
     * committed json/hash 都存在但 hash 不匹配 → Corrupted。
     */
    @Test
    fun getCommittedManifestStrict_hashMismatch_returnsCorrupted() {
        val json =
            """
            {
                "backend": "media_store",
                "committedManifestJson": "{}",
                "committedManifestHash": "wrong-hash",
                "projects": {"proj1": {"v1/ch1": {"uri": "u", "relativePath": "p", "revision": 1, "contentHash": "h"}}}
            }
            """.trimIndent()
        writeStateJson(json)
        val result = store.getCommittedManifestStrict()
        assertTrue("hash 不匹配 → Corrupted", result is CommittedManifestReadResult.Corrupted)
    }

    /**
     * committed json/hash 都存在且 hash 正确 → Found。
     */
    @Test
    fun getCommittedManifestStrict_validHash_returnsFound() {
        val json =
            """
            {
                "backend": "media_store",
                "committedManifestJson": "{\"schemaVersion\":1,\"revision\":1,\"updatedAt\":\"2024-01-01T00:00:00Z\",\"projects\":[]}",
                "committedManifestHash": "${computeContentHash(
                """{"schemaVersion":1,"revision":1,"updatedAt":"2024-01-01T00:00:00Z","projects":[]}""",
            )}",
                "projects": {"proj1": {"v1/ch1": {"uri": "u", "relativePath": "p", "revision": 1, "contentHash": "h"}}}
            }
            """.trimIndent()
        writeStateJson(json)
        val result = store.getCommittedManifestStrict()
        assertTrue("valid hash → Found", result is CommittedManifestReadResult.Found)
    }

    /**
     * projects 损坏但 committed json/hash 正常 → getCommittedManifestStrict 返回 Corrupted
     *（不再只判 manifest OK 就放行）。
     *
     * 这是 comment 5578472936 指出的核心问题：baseline 正常但 projects 损坏，
     * 旧代码只判 manifest，现在 decodeStateRootStrict 整体判。
     */
    @Test
    fun getCommittedManifestStrict_validManifestButCorruptedProjects_returnsCorrupted() {
        val validManifest = """{"schemaVersion":1,"revision":1,"updatedAt":"2024-01-01T00:00:00Z","projects":[]}"""
        val json =
            """
            {
                "backend": "media_store",
                "committedManifestJson": "$validManifest",
                "committedManifestHash": "${computeContentHash(validManifest)}",
                "projects": {"p1": "bad-value"}
            }
            """.trimIndent()
        writeStateJson(json)
        val result = store.getCommittedManifestStrict()
        assertTrue(
            "projects 损坏但 manifest 正常 → Corrupted（不再放行）",
            result is CommittedManifestReadResult.Corrupted,
        )
    }

    /**
     * 有旧 mirror state 字段但无 committed baseline → NeedsMigration。
     */
    @Test
    fun getCommittedManifestStrict_oldStateNoCommitted_returnsNeedsMigration() {
        writeStateJson("""{"backend": "media_store", "manifestUri": "content://m"}""")
        val result = store.getCommittedManifestStrict()
        assertEquals(CommittedManifestReadResult.NeedsMigration, result)
    }

    /**
     * 全空 state → NotExists。
     */
    @Test
    fun getCommittedManifestStrict_emptyState_returnsNotExists() {
        writeStateJson("""{"backend": "media_store"}""")
        val result = store.getCommittedManifestStrict()
        assertEquals(CommittedManifestReadResult.NotExists, result)
    }

    /**
     * committed json 存在但 hash 缺失 → Corrupted（partial state）。
     */
    @Test
    fun getCommittedManifestStrict_partialState_returnsCorrupted() {
        writeStateJson("""{"backend": "media_store", "committedManifestJson": "{}"}""")
        val result = store.getCommittedManifestStrict()
        assertTrue("json 有 hash 无 → Corrupted", result is CommittedManifestReadResult.Corrupted)
    }

    // ══════════════════════════════════════════════════════════════════════
    // saveRestoredState — 旧 state 损坏时拒绝
    // ══════════════════════════════════════════════════════════════════════

    /**
     * 已有 state 但 projects 损坏 → saveRestoredState 返回 false（不覆盖）。
     */
    @Test
    fun saveRestoredState_corruptedExistingState_returnsFalse() {
        writeStateJson("""{"backend": "media_store", "projects": {"p1": 42}}""")
        val result =
            store.saveRestoredState(
                manifestUri = "content://m",
                chapterEntries = emptyMap(),
            )
        assertFalse("旧 state 损坏 → false（不覆盖）", result)
    }

    /**
     * 已有 state 但 treeUri 类型损坏 → saveRestoredState 返回 false。
     */
    @Test
    fun saveRestoredState_corruptedTreeUri_returnsFalse() {
        writeStateJson("""{"backend": "media_store", "treeUri": 123}""")
        val result =
            store.saveRestoredState(
                manifestUri = "content://m",
                chapterEntries = emptyMap(),
            )
        assertFalse("旧 treeUri 损坏 → false", result)
    }

    /**
     * 已有 state 不存在 → saveRestoredState 从空 root 开始，正常写入。
     */
    @Test
    fun saveRestoredState_notExists_writesNewState() {
        val dir =
            File(ApplicationProvider.getApplicationContext<android.app.Application>().noBackupFilesDir, "sujian-mirror")
        File(dir, "state.json").delete()
        val result =
            store.saveRestoredState(
                manifestUri = "content://m",
                chapterEntries = emptyMap(),
            )
        assertTrue("不存在 → true（新写入）", result)
        // 验证写入后的 backend
        assertEquals(MirrorBackend.DOCUMENT_TREE, store.getBackend())
    }

    /**
     * state.json 损坏（无效 JSON）→ saveRestoredState 返回 false。
     */
    @Test
    fun saveRestoredState_corruptedJson_returnsFalse() {
        val dir =
            File(ApplicationProvider.getApplicationContext<android.app.Application>().noBackupFilesDir, "sujian-mirror")
        dir.mkdirs()
        val file = File(dir, "state.json")
        file.writeText("not valid json {{{", Charsets.UTF_8)
        val result =
            store.saveRestoredState(
                manifestUri = "content://m",
                chapterEntries = emptyMap(),
            )
        assertFalse("JSON 损坏 → false", result)
    }
}
