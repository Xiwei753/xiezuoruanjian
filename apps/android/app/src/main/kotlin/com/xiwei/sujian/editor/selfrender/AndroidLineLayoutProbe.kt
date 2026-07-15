package com.xiwei.sujian.editor.selfrender

interface ClusterStabilityInfo {
    val documentByteStart: Int
    val documentByteEnd: Int
    val textDirection: Int
    val shapingIdentity: String
}

data class AndroidClusterLayoutProbe(
    override val documentByteStart: Int,
    override val documentByteEnd: Int,
    val visualWidth: Float,
    val isRtl: Boolean,
    override val shapingIdentity: String
) : ClusterStabilityInfo {
    override val textDirection: Int get() = if (isRtl) 1 else 0
}

data class AndroidLineLayoutProbe(
    val visualLineOrdinal: Int,
    val documentByteStart: Int,
    val documentByteEnd: Int,
    val lineStartUtf16: Int,
    val lineEndUtf16: Int,
    val lineTop: Int,
    val lineBottom: Int,
    val clusters: List<AndroidClusterLayoutProbe>,
    val breakIdentity: String
)
