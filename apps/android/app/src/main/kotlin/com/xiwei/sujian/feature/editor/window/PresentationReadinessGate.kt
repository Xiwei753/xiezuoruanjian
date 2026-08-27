package com.xiwei.sujian.feature.editor.window

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first

/**
 * #640 B：presentation-ready 几何状态闸门 — 纯 Kotlin seam，可单测。
 *
 * 封装 (ready, generation) 两个 StateFlow + replacement-aware await 逻辑。
 * [EditorWindowHost] 持有唯一实例并委托 ready 状态管理给它；不引入第二套状态源 —
 * ready/generation 的唯一写入点在此类，EditorWindowHost 仅通过本类公开方法驱动。
 *
 * - [publishReady]：View layout 就绪时发布带当前真实几何的 ready。
 * - [invalidateGeometry]：尺寸变化时先使旧几何 ready 失效（不推进代次，同一 target）。
 * - [invalidateAndAdvance]：target 替换/关闭/解绑/窗口释放时使旧 ready 失效并推进代次。
 * - [awaitPresentationReady]：replacement-aware，代次偏离立即返回 false，不会永久挂住。
 */
internal class PresentationReadinessGate {
    private val _ready = MutableStateFlow<EditorPresentationReady?>(null)
    val ready: StateFlow<EditorPresentationReady?> = _ready.asStateFlow()

    private val _generation = MutableStateFlow(0L)
    val generation: StateFlow<Long> = _generation.asStateFlow()

    /** 检查指定 target 的 presentation 是否已就绪（含几何 width/height > 0）。 */
    fun isReady(targetId: String): Boolean {
        val r = _ready.value ?: return false
        return r.targetId == targetId && r.widthPx > 0 && r.heightPx > 0
    }

    /** View layout 就绪时发布带当前真实几何的 ready（width/height 必须 > 0）。 */
    fun publishReady(
        targetId: String,
        widthPx: Int,
        heightPx: Int,
    ) {
        if (widthPx <= 0 || heightPx <= 0) return
        _ready.value = EditorPresentationReady(targetId, widthPx, heightPx)
    }

    /** 尺寸变化时先使旧几何 ready 失效（不推进代次，同一 target 仍在等待）。 */
    fun invalidateGeometry() {
        _ready.value = null
    }

    /** target 替换/关闭/解绑/窗口释放时使旧 ready 失效并推进代次。 */
    fun invalidateAndAdvance() {
        _ready.value = null
        _generation.value = _generation.value + 1L
    }

    /**
     * 指定 target 关闭/解绑时：若当前 ready 属于此 target 则清掉（避免旧 onDispose 清错新 target），
     * 并推进代次让等待此 target 的 await 快速返回 false。
     *
     * 与 [invalidateAndAdvance] 的区别：不清掉属于别的 target 的 ready，
     * 避免 detachView(A) 误清新 target B 的 ready。
     */
    fun invalidateTarget(targetId: String) {
        if (_ready.value?.targetId == targetId) {
            _ready.value = null
        }
        _generation.value = _generation.value + 1L
    }

    /**
     * Replacement-aware await：返回 true 仅当当前几何 ready 命中此 target；
     * 代次偏离（target 被替换/关闭）时立即返回 false，不会让 [first] 永久等不到。
     */
    suspend fun awaitPresentationReady(targetId: String): Boolean {
        val startGeneration = _generation.value
        _ready.value?.let { r ->
            if (r.targetId == targetId && r.widthPx > 0 && r.heightPx > 0 &&
                _generation.value == startGeneration
            ) {
                return true
            }
        }
        if (_generation.value != startGeneration) return false
        return combine(_ready, _generation) { ready, gen ->
            when {
                gen != startGeneration -> false
                ready != null && ready.targetId == targetId &&
                    ready.widthPx > 0 && ready.heightPx > 0 -> true
                else -> null
            }
        }.filterNotNull().first()
    }
}
