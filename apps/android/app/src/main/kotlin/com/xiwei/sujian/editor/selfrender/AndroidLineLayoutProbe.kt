package com.xiwei.sujian.editor.selfrender

data class AndroidClusterLayoutProbe(
    val documentByteStart: Int,
    val documentByteEnd: Int,
    val visualWidth: Float,
    val isRtl: Boolean,
    val shapingIdentity: String
)

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
