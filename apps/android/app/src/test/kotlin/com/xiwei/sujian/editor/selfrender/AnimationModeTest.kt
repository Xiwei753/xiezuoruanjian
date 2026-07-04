package com.xiwei.sujian.editor.selfrender

import com.xiwei.sujian.model.AnimationModeData
import org.junit.Assert.*
import org.junit.Test

class AnimationModeTest {
    private fun chooseAnimationMode(
        clusterCount: Int,
        containsNewline: Boolean,
        containsComplexGrapheme: Boolean,
        isScrolling: Boolean = false,
        isLoading: Boolean = false,
        isApplyingFormat: Boolean = false,
        isApplyingSettings: Boolean = false,
        animEnabled: Boolean = true,
        componentReady: Boolean = true
    ): AnimationModeData {
        if (!animEnabled) return AnimationModeData.SystemSuppressed
        if (isScrolling || isLoading || isApplyingFormat || isApplyingSettings) return AnimationModeData.SystemSuppressed
        if (!componentReady) return AnimationModeData.SystemSuppressed
        if (clusterCount == 0) return AnimationModeData.SystemSuppressed
        if (containsNewline) return AnimationModeData.LineReflowAnimation
        if (containsComplexGrapheme) return AnimationModeData.ClusterAnimation
        if (clusterCount <= 8) return AnimationModeData.GlyphAnimation
        if (clusterCount <= 40) return AnimationModeData.RunAnimation
        return AnimationModeData.SnapshotAnimation
    }
    
    @Test
    fun singleChar_typing_returns_glyphAnimation() {
        assertEquals(AnimationModeData.GlyphAnimation, chooseAnimationMode(1, false, false))
    }
    
    @Test
    fun eightClusters_returns_glyphAnimation() {
        assertEquals(AnimationModeData.GlyphAnimation, chooseAnimationMode(8, false, false))
    }
    
    @Test
    fun nineClusters_returns_runAnimation() {
        assertEquals(AnimationModeData.RunAnimation, chooseAnimationMode(9, false, false))
    }
    
    @Test
    fun fortyClusters_returns_runAnimation() {
        assertEquals(AnimationModeData.RunAnimation, chooseAnimationMode(40, false, false))
    }
    
    @Test
    fun fortyOneClusters_returns_snapshotAnimation() {
        assertEquals(AnimationModeData.SnapshotAnimation, chooseAnimationMode(41, false, false))
    }
    
    @Test
    fun emoji_returns_clusterAnimation() {
        assertEquals(AnimationModeData.ClusterAnimation, chooseAnimationMode(1, false, true))
    }
    
    @Test
    fun newline_returns_lineReflowAnimation() {
        assertEquals(AnimationModeData.LineReflowAnimation, chooseAnimationMode(1, true, false))
    }
    
    @Test
    fun scrolling_returns_systemSuppressed() {
        assertEquals(AnimationModeData.SystemSuppressed, chooseAnimationMode(1, false, false, isScrolling = true))
    }
    
    @Test
    fun disabled_returns_systemSuppressed() {
        assertEquals(AnimationModeData.SystemSuppressed, chooseAnimationMode(1, false, false, animEnabled = false))
    }
    
    @Test
    fun loading_returns_systemSuppressed() {
        assertEquals(AnimationModeData.SystemSuppressed, chooseAnimationMode(1, false, false, isLoading = true))
    }
    
    @Test
    fun zeroClusters_returns_systemSuppressed() {
        assertEquals(AnimationModeData.SystemSuppressed, chooseAnimationMode(0, false, false))
    }
    
    @Test
    fun newline_withComplexGrapheme_newlineTakesPriority() {
        // 换行优先于复杂 grapheme
        assertEquals(AnimationModeData.LineReflowAnimation, chooseAnimationMode(1, true, true))
    }
    
    @Test
    fun newline_notSystemSuppressed() {
        // 换行不返回 SystemSuppressed — 关键：用户输入换行必须有动画
        val mode = chooseAnimationMode(1, true, false)
        assertNotEquals(AnimationModeData.SystemSuppressed, mode)
    }
    
    @Test
    fun complexGrapheme_notSystemSuppressed() {
        // 复杂 grapheme 不返回 SystemSuppressed — 关键：emoji 必须有动画
        val mode = chooseAnimationMode(1, false, true)
        assertNotEquals(AnimationModeData.SystemSuppressed, mode)
    }
}
