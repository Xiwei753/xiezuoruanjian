package com.xiwei.sujian.ui

/**
 * #595 二：同步合并事件发射去重 — 每个章节只发射一次同一 fileHash 的
 * [com.xiwei.sujian.editor.v2.coordinator.EditorDocumentUpdate.SyncMerged]。
 *
 * 根因：`checkSyncMergedChapter` 的发射守卫使用 `uiState.chapterHash`，而同步合并
 * 应用后该 hash 不会更新（会话层 `lastRepositoryHash` 与 ViewModel 的 chapterHash
 * 是两份事实），导致每个同步周期都会重新发射同一合并事件（每次都带新的
 * contentVersion）。reducer 的 lastRepositoryHash 守卫能拦截这些重复事件，
 * 但发射端仍应去重，避免无意义的重复磁盘读取与事件噪音。
 *
 * 语义：
 * - [shouldEmit] 只在真正发射时记录 hash；同一 hash 的后续请求返回 false；
 * - [reset] 在章节提交（switchChapterLocked 成功提交 / initChapter）时调用，
 *   使重新进入章节后同一 hash 可以再次发射（重新进入时正文由
 *   RepositoryLoaded 事件装载，SyncMerged 只需报告新的磁盘变化）。
 *
 * 线程安全：observer 运行在 Dispatchers.IO 单协程内，无需加锁。
 */
class SyncMergeEmitDedup {
    private var lastEmittedHash: String? = null

    /** 返回 true 表示该 hash 尚未发射过，并记录之；返回 false 表示重复发射请求。 */
    fun shouldEmit(fileHash: String): Boolean {
        if (fileHash.isEmpty()) return false
        if (lastEmittedHash == fileHash) return false
        lastEmittedHash = fileHash
        return true
    }

    /** 章节提交/切换时重置 — 重新进入章节后允许重新发射同一 hash。 */
    fun reset() {
        lastEmittedHash = null
    }
}
