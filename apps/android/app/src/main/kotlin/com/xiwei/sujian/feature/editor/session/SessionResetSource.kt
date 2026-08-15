package com.xiwei.sujian.feature.editor.session

/**
 * 会话重置来源。
 */
enum class SessionResetSource {
    LOCAL_CONTENT_CHANGED,
    EXTERNAL,
    CHAPTER_SWITCH,
}

/**
 * #624 评论17 问题4：persistent reset 的纯数据前置条件 — readSession 捕获后，
 * 锁外 createSession/querySnapshotForSession，再进 mutateSession 内重新校验
 * precondition 完全一致才 swap candidate。任一字段不一致说明锁外期间同 target
 * 的 revision/session/epoch 已前进，candidate 必须丢弃，不得覆盖当前新 session。
 */
data class SessionResetPrecondition(
    val targetId: String,
    val oldSessionId: ULong,
    val oldRevision: Long,
    val leaseEpoch: Long,
)
