package com.xiwei.sujian.editor.v2.visual

data class AnimationStateSnapshot(
    val transactionId: Long,
    val operationKind: String,
    val animationMode: String,
    val oldAffectedRanges: List<Pair<Int, Int>>,
    val newAffectedRanges: List<Pair<Int, Int>>,
    val progress: Float,
    val sliceRoles: List<SliceRole>,
    val cursorTransition: CursorTransitionSnapshot?,
    val ownedResourceCount: Int,
    val transactionState: TransactionState
)

data class CursorTransitionSnapshot(
    val fromX: Float,
    val fromY: Float,
    val fromHeight: Float,
    val toX: Float,
    val toY: Float,
    val toHeight: Float,
    val shouldAnimate: Boolean
)
