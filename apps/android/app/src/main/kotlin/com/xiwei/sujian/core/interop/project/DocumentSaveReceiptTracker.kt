package com.xiwei.sujian.core.interop.project

/**
 * #595 七/四：指定 target 的持久化屏障 — 每个 target 的保存状态收口。
 *
 * 替代旧全局 lastSaveResult（跨章节共享的假成功）：上次保存成功后，即使
 * 当前正文从未写入磁盘，flush 也会返回 true，同步继续使用磁盘里的旧正文。
 *
 * 本跟踪器按 target 记录"该 token 的正文已得到保存回执"：
 *
 * ```kotlin
 * record(token)   // 保存/清空/外部落盘应用后记录
 * canFlush(token, committedContentHash)
 * ```
 *
 * [canFlush] 只有在满足全部条件时才放行同步：
 * - 该 target 存在保存回执；
 * - 回执的 coreSessionId 与 token 一致（不同 Rust session 的 revision 数值
 *   可以相同，不得跨 session 假成功）；
 * - 回执的 inputEpoch 与 token 一致（章节切换后旧保存结果不匹配新 epoch）；
 * - 回执的 rustRevision 与 token 一致（保存后又输入 → revision 不一致
 *   → flush 失败，同步中止）；
 * - 回执的 textHash 与会话层 committedVersion 一致（版本已被外部事实
 *   推进但磁盘未同步保存 → 不一致 → flush 失败）。
 *
 * 线程安全：由调用方（EditorViewModel 保存 actor，单线程顺序处理）串行访问。
 */
class DocumentSaveReceiptTracker {
    /**
     * #595 四：保存令牌 — 携带完整的文档身份信息，不只比较 revision 数字。
     *
     * 保存返回后由同一 reducer 判断：
     * - 当前 token 完全一致 → 标记 Saved、推进版本、清 dirty；
     * - 当前 target/session/epoch/revision 已变化 → 只记录旧版本确实落盘，
     *   当前文档保持 Unsaved，立即排队保存当前最新 token。
     */
    data class SaveToken(
        val operationId: Long,
        val targetId: String,
        val coreSessionId: ULong,
        val inputEpoch: Long,
        val rustRevision: Long,
        val textHash: String,
    )

    private val receipts = mutableMapOf<String, SaveToken>()

    /** 记录一次成功保存/清空/外部落盘应用的回执。 */
    fun record(token: SaveToken) {
        receipts[token.targetId] = token
    }

    /** 当前回执（无保存记录为 null）。 */
    fun receipt(targetId: String): SaveToken? = receipts[targetId]

    /**
     * flush 屏障 — [token] 为请求 flush 时的完整保存令牌，
     * [committedContentHash] 为会话层 committedVersion 的 contentHash
     * （空/null 表示版本事实尚未建立，跳过 hash 一致性检查）。
     */
    fun canFlush(
        token: SaveToken,
        committedContentHash: String?,
    ): Boolean {
        val receipt = receipts[token.targetId] ?: return false
        if (receipt.coreSessionId != token.coreSessionId) return false
        if (receipt.inputEpoch != token.inputEpoch) return false
        if (receipt.rustRevision != token.rustRevision) return false
        if (committedContentHash.isNullOrEmpty()) return true
        return receipt.textHash == committedContentHash
    }

    /** 章节切换/关闭时清理该 target 的旧回执（跨章节假成功防护）。 */
    fun clear(targetId: String) {
        receipts.remove(targetId)
    }

    fun clearAll() {
        receipts.clear()
    }
}
