package com.xiwei.sujian.storage.mirror

import androidx.core.util.AtomicFile
import androidx.test.core.app.ApplicationProvider
import org.json.JSONObject
import org.junit.Assert.assertEquals
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
 * #649 评论 5578666118：backend / treeUri 仍没有真正 strict 的回归测试。
 *
 * 验证 comment 5578666118 指出的 fail-open 路径已修复：
 * - backend=""（空字符串）不再被当成 null 回退到 MEDIA_STORE
 * - backend="document_tree" + treeUri=""（空字符串）不再被接受
 * - backend="document_tree" + treeUri 缺失 → failure
 * - manifestUri=""（空字符串）不再被接受
 * - backend 字段不存在 → 向后兼容 MEDIA_STORE
 * - backend="document_tree" + treeUri 合法 URI → success
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
@Suppress("TooManyFunctions", "StringLiteralDuplication")
class Issue649Comment5578666118ReproTest {

    private lateinit var store: ReadableMirrorStateStore

    @Before
    fun setUp() {
        val context = ApplicationProvider.getApplicationContext<android.app.Application>()
        store = ReadableMirrorStateStore(context)
    }

    // ── helper ──

    private fun writeStateJson(json: String) {
        val dir = File(ApplicationProvider.getApplicationContext<android.app.Application>().noBackupFilesDir, "sujian-mirror")
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

    // ══════════════════════════════════════════════════════════════════════
    // backend 解析严格性
    // ══════════════════════════════════════════════════════════════════════

    /**
     * #649 评论 5578666118：backend 字段不存在 → 向后兼容 MEDIA_STORE（合法旧版 state）。
     */
    @Test
    fun readSnapshotStrict_backendMissing_compatMediaStore() {
        writeStateJson("""{}""")
        val result = store.readSnapshotStrict()
        assertTrue("backend 缺失 → success（兼容 MEDIA_STORE）", result.isSuccess)
        val snapshot = result.getOrNull()!!
        assertEquals(MirrorBackend.MEDIA_STORE, snapshot.backend)
        assertNull(snapshot.treeUri)
        assertNull(snapshot.manifestUri)
    }

    /**
     * #649 评论 5578666118：backend=""（空字符串）→ 损坏，failure。
     *
     * 旧代码用 `optString().takeIf { it.isNotEmpty() }` 把空字符串当 null，
     * 然后回退到 MEDIA_STORE。修复后空字符串被拒绝。
     */
    @Test
    fun readSnapshotStrict_backendEmptyString_failure() {
        writeStateJson("""{"backend": ""}""")
        val result = store.readSnapshotStrict()
        assertTrue("backend=\"\" → failure（损坏状态）", result.isFailure)
    }

    /**
     * #649 评论 5578666118：backend="document_tree" + treeUri 缺失 → failure。
     *
     * Router 里 requireNotNull(treeUri) 会拦截，但 decodeStateRootStrict 就应该
     * 在更早的地方拦住，不把损坏状态暴露给下游。
     */
    @Test
    fun readSnapshotStrict_documentTreeMissingTreeUri_failure() {
        writeStateJson("""{"backend": "document_tree"}""")
        val result = store.readSnapshotStrict()
        assertTrue("document_tree 无 treeUri → failure", result.isFailure)
    }

    /**
     * #649 评论 5578666118：backend="document_tree" + treeUri=""（空字符串）→ failure。
     *
     * 旧代码接受空字符串 treeUri，Router 的 requireNotNull(treeUri) 不会拦截
     * （非 null 但空字符串），Uri.parse("") 继续往下走导致不可预测行为。
     */
    @Test
    fun readSnapshotStrict_documentTreeEmptyTreeUri_failure() {
        writeStateJson("""{"backend": "document_tree", "treeUri": ""}""")
        val result = store.readSnapshotStrict()
        assertTrue("document_tree + treeUri=\"\" → failure", result.isFailure)
    }

    /**
     * #649 评论 5578666118：backend="document_tree" + treeUri 合法 URI → success。
     */
    @Test
    fun readSnapshotStrict_documentTreeValidTreeUri_success() {
        writeStateJson("""{"backend": "document_tree", "treeUri": "content://com.android.providers.downloads.documents/tree/primary%3ADownload%2FSujian"}""")
        val result = store.readSnapshotStrict()
        assertTrue("document_tree + 合法 treeUri → success", result.isSuccess)
        val snapshot = result.getOrNull()!!
        assertEquals(MirrorBackend.DOCUMENT_TREE, snapshot.backend)
        assertEquals("content://com.android.providers.downloads.documents/tree/primary%3ADownload%2FSujian", snapshot.treeUri)
    }

    // ══════════════════════════════════════════════════════════════════════
    // manifestUri 严格性
    // ══════════════════════════════════════════════════════════════════════

    /**
     * #649 评论 5578666118：manifestUri=""（空字符串）→ failure。
     *
     * 旧代码 accept 空字符串 manifestUri，"没有 manifest"应该用字段不存在表达。
     */
    @Test
    fun readSnapshotStrict_manifestUriEmptyString_failure() {
        writeStateJson("""{"backend": "media_store", "manifestUri": ""}""")
        val result = store.readSnapshotStrict()
        assertTrue("manifestUri=\"\" → failure（损坏状态）", result.isFailure)
    }

    /**
     * manifestUri 字段不存在 → 合法（manifestUri=null）。
     */
    @Test
    fun readSnapshotStrict_manifestUriMissing_success() {
        writeStateJson("""{"backend": "media_store"}""")
        val result = store.readSnapshotStrict()
        assertTrue(result.isSuccess)
        assertNull(result.getOrNull()!!.manifestUri)
    }

    // ══════════════════════════════════════════════════════════════════════
    // getAllChapterEntriesStrict — 同样走 decodeStateRootStrict
    // ══════════════════════════════════════════════════════════════════════

    /**
     * getAllChapterEntriesStrict 也应拒绝 backend=""。
     */
    @Test
    fun getAllChapterEntriesStrict_backendEmpty_failure() {
        writeStateJson("""{"backend": ""}""")
        val result = store.getAllChapterEntriesStrict()
        assertTrue("getAllChapterEntriesStrict: backend=\"\" → failure", result.isFailure)
    }

    // ══════════════════════════════════════════════════════════════════════
    // getCommittedManifestStrict — 同样走 decodeStateRootStrict
    // ══════════════════════════════════════════════════════════════════════

    /**
     * getCommittedManifestStrict 也应拒绝 backend=""。
     */
    @Test
    fun getCommittedManifestStrict_backendEmpty_returnsCorrupted() {
        writeStateJson("""{"backend": ""}""")
        val result = store.getCommittedManifestStrict()
        assertTrue(
            "getCommittedManifestStrict: backend=\"\" → Corrupted",
            result is CommittedManifestReadResult.Corrupted,
        )
    }

    // ══════════════════════════════════════════════════════════════════════
    // saveRestoredState — 旧 state 含 backend="" → 拒绝覆盖
    // ══════════════════════════════════════════════════════════════════════

    /**
     * #649 评论 5578666118：已有 state 但 backend="" → saveRestoredState 返回 false。
     */
    @Test
    fun saveRestoredState_backendEmpty_returnsFalse() {
        writeStateJson("""{"backend": ""}""")
        val result = store.saveRestoredState(
            manifestUri = "content://m",
            chapterEntries = emptyMap(),
        )
        assertTrue("旧 state backend=\"\" → false（decodeStateRootStrict 拒绝）", !result)
    }
}
