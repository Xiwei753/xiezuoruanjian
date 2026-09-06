package com.xiwei.sujian.storage.mirror

import kotlinx.serialization.Serializable
import org.json.JSONObject

/**
 * ReadableMirrorManifest — Download/Sujian 镜像的共享清单格式。
 *
 * #649 评论 5560685734 要求 4：Publisher 和 Restorer 共用同一个格式，
 * 避免两套 schema。manifest 描述镜像中 project/volume/chapter 的树形结构与
 * 每章正文的 contentHash，便于恢复时校验完整性。
 *
 * ## 磁盘布局
 * ```
 * Download/Sujian/
 * ├── _meta/
 * │   └── manifest.json   # 本清单
 * └── projects/
 *     └── <projectId>/
 *         └── volumes/
 *             └── <volumeId>/
 *                 └── chapters/
 *                     └── <chapterId>.md
 * ```
 *
 * ## JSON Schema
 * ```json
 * {
 *   "schemaVersion": 1,
 *   "revision": 1694123456789,
 *   "updatedAt": "2026-09-07T00:00:00Z",
 *   "projects": [
 *     {
 *       "id": "<uuid>",
 *       "title": "作品标题",
 *       "order": 0,
 *       "revision": 1694123456789,
 *       "updatedAt": "2026-09-07T00:00:00Z",
 *       "volumes": [
 *         {
 *           "id": "<uuid>",
 *           "title": "卷标题",
 *           "order": 0,
 *           "revision": 1694123456789,
 *           "updatedAt": "2026-09-07T00:00:00Z",
 *           "chapters": [
 *             {
 *               "id": "<uuid>",
 *               "title": "章节标题",
 *               "order": 0,
 *               "revision": 1694123456789,
 *               "updatedAt": "2026-09-07T00:00:00Z",
 *               "contentFile": "projects/<pid>/volumes/<vid>/chapters/<cid>.md",
 *               "contentHash": "sha256:<hex>"
 *             }
 *           ]
 *         }
 *       ]
 *     }
 *   ]
 * }
 * ```
 *
 * ## 字段说明
 * - `schemaVersion`：清单格式版本，当前为 1。用于后续兼容升级。
 * - `revision`：镜像整体修订号（毫秒级时间戳），与 Core 的 revision 一致。
 * - `updatedAt`：镜像最后更新时间（ISO-8601 UTC）。
 * - `contentHash`：章节正文的 SHA-256 哈希（前缀 `sha256:`），恢复时校验文件完整性。
 *   空正文为 `sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`。
 */
@Serializable
data class MirrorManifest(
    val schemaVersion: Int = 1,
    val revision: Long,
    val updatedAt: String,
    val projects: List<MirrorProject>,
)

@Serializable
data class MirrorProject(
    val id: String,
    val title: String,
    val order: Int,
    val revision: Long,
    val updatedAt: String,
    val volumes: List<MirrorVolume>,
)

@Serializable
data class MirrorVolume(
    val id: String,
    val title: String,
    val order: Int,
    val revision: Long,
    val updatedAt: String,
    val chapters: List<MirrorChapter>,
)

@Serializable
data class MirrorChapter(
    val id: String,
    val title: String,
    val order: Int,
    val revision: Long,
    val updatedAt: String,
    /** 内容文件相对路径，如 `projects/<id>/volumes/<vid>/chapters/<cid>.md`。 */
    val contentFile: String,
    /** 正文 SHA-256 哈希，前缀 `sha256:`；空正文为 64 个零的空哈希。 */
    val contentHash: String,
)

/**
 * 计算正文的 SHA-256 哈希（前缀 `sha256:`）。
 *
 * 空字符串返回标准空哈希：`sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`。
 */
fun computeContentHash(content: String): String {
    if (content.isEmpty()) {
        return "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    }
    val bytes = content.toByteArray(Charsets.UTF_8)
    val digest = java.security.MessageDigest.getInstance("SHA-256")
    val hashBytes = digest.digest(bytes)
    val hex = hashBytes.joinToString("") { "%02x".format(it) }
    return "sha256:$hex"
}

/**
 * 验证正文哈希是否匹配。
 */
fun verifyContentHash(content: String, expectedHash: String): Boolean {
    if (expectedHash.isEmpty()) return false
    if (expectedHash == "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855") {
        return content.isEmpty()
    }
    return computeContentHash(content) == expectedHash
}
