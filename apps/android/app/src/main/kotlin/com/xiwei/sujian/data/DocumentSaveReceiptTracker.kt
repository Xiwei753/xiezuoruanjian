package com.xiwei.sujian.data

/**
 * #595 七：指定 target 和 revision 的持久化屏障 — 每个 target 的保存状态收口。
 *
 * 替代旧全局 lastSaveResult（跨章节共享的假成功）：上次保存成功后，即使
 * 当前正文从未写入磁盘，flush 也会返回 true，同步继续使用磁盘里的旧正文。
 *
 * 本跟踪器按 target 记录"该 revision 的正文已得到保存回执"：
 *
 * ```kotlin
 * record(targetId, revision, contentHash)   // 保存/清空/外部落盘应用后记录
 * canFlush(targetId, requiredRevision, committedContentHash)
 * ```
 *
 * [canFlush] 只有在满足全部条件时才放行同步：
 * - 该 target 存在保存回执；
 * - 回执的 revision 与屏幕正文 revision 一致（保存后又输入 → revision 不一致
 *   → flush 失败，同步中止）；
 * - 回执的 contentHash 与会话层 committedVersion 一致（版本已被外部事实
 *   推进但磁盘未同步保存 → 不一致 → flush 失败）。
 *
 * 线程安全：由调用方（EditorViewModel 保存 actor，单线程顺序处理）串行访问。
 */
class DocumentSaveReceiptTracker {
    /** 保存回执 — revision 是入队保存时的 Rust session revision。 */
    data class SaveReceipt(
        val revision: Long,
        val contentHash: String,
    )

    private val receipts = mutableMapOf<String, SaveReceipt>()

    /** 记录一次成功保存/清空/外部落盘应用的回执。 */
    fun record(targetId: String, revision: Long, contentHash: String) {
        receipts[targetId] = SaveReceipt(revision, contentHash)
    }

    /** 当前回执（无保存记录为 null）。 */
    fun receipt(targetId: String): SaveReceipt? = receipts[targetId]

    /**
     * flush 屏障 — [requiredRevision] 为请求 flush 时屏幕正文的 Rust revision，
     * [committedContentHash] 为会话层 committedVersion 的 contentHash
     * （空/null 表示版本事实尚未建立，跳过 hash 一致性检查）。
     */
    fun canFlush(
        targetId: String,
        requiredRevision: Long,
        committedContentHash: String?,
    ): Boolean {
        val receipt = receipts[targetId] ?: return false
        if (receipt.revision != requiredRevision) return false
        if (committedContentHash.isNullOrEmpty()) return true
        return receipt.contentHash == committedContentHash
    }

    /** 章节切换/关闭时清理该 target 的旧回执（跨章节假成功防护）。 */
    fun clear(targetId: String) {
        receipts.remove(targetId)
    }

    fun clearAll() {
        receipts.clear()
    }
}
