package com.xiwei.sujian.ui

import org.junit.Assert.*
import org.junit.Test

class ComplexGraphemeAnimationTest {

    @Test
    fun simpleAscii_notComplex() {
        val anim = OverlayAnim(
            insertedStart = 0,
            insertedText = "abc",
            startX = 0f,
            startY = 0f,
            durationMs = 100L,
            isDeletion = false
        )
        assertFalse("Simple ASCII should not skip glyph animation", anim.skipGlyphAnimation)
    }

    @Test
    fun simpleChinese_notComplex() {
        val anim = OverlayAnim(
            insertedStart = 0,
            insertedText = "你",
            startX = 0f,
            startY = 0f,
            durationMs = 100L,
            isDeletion = false
        )
        assertFalse("Single Chinese char should not skip glyph animation", anim.skipGlyphAnimation)
    }

    @Test
    fun emoji_isComplex() {
        val anim = OverlayAnim(
            insertedStart = 0,
            insertedText = "😀",
            startX = 0f,
            startY = 0f,
            durationMs = 100L,
            isDeletion = false
        )
        assertTrue("Emoji should skip glyph animation", anim.skipGlyphAnimation)
    }

    @Test
    fun zwjEmoji_isComplex() {
        // 👨‍👩‍👧 = man + ZWJ + woman + ZWJ + girl
        val anim = OverlayAnim(
            insertedStart = 0,
            insertedText = "👨‍👩‍👧",
            startX = 0f,
            startY = 0f,
            durationMs = 100L,
            isDeletion = false
        )
        assertTrue("ZWJ emoji should skip glyph animation", anim.skipGlyphAnimation)
    }

    @Test
    fun combiningMark_isComplex() {
        // e + combining acute accent
        val anim = OverlayAnim(
            insertedStart = 0,
            insertedText = "e\u0301",
            startX = 0f,
            startY = 0f,
            durationMs = 100L,
            isDeletion = false
        )
        assertTrue("Combining mark should skip glyph animation", anim.skipGlyphAnimation)
    }

    @Test
    fun variationSelector_isComplex() {
        // digit one + variation selector-16
        val anim = OverlayAnim(
            insertedStart = 0,
            insertedText = "1\uFE0F",
            startX = 0f,
            startY = 0f,
            durationMs = 100L,
            isDeletion = false
        )
        assertTrue("Variation selector should skip glyph animation", anim.skipGlyphAnimation)
    }

    @Test
    fun emptyText_notComplex() {
        val anim = OverlayAnim(
            insertedStart = 0,
            insertedText = "",
            startX = 0f,
            startY = 0f,
            durationMs = 100L,
            isDeletion = false
        )
        assertFalse("Empty text should not skip glyph animation", anim.skipGlyphAnimation)
    }

    @Test
    fun mixedChineseAndAscii_notComplex() {
        val anim = OverlayAnim(
            insertedStart = 0,
            insertedText = "a你b",
            startX = 0f,
            startY = 0f,
            durationMs = 100L,
            isDeletion = false
        )
        assertFalse("Mixed Chinese+ASCII without complex grapheme should not skip", anim.skipGlyphAnimation)
    }

    @Test
    fun chinesePunctuation_notComplex() {
        val anim = OverlayAnim(
            insertedStart = 0,
            insertedText = "，。！？",
            startX = 0f,
            startY = 0f,
            durationMs = 100L,
            isDeletion = false
        )
        assertFalse("Chinese punctuation should not skip glyph animation", anim.skipGlyphAnimation)
    }

    @Test
    fun emojiAfterChinese_isComplex() {
        val anim = OverlayAnim(
            insertedStart = 0,
            insertedText = "你😀",
            startX = 0f,
            startY = 0f,
            durationMs = 100L,
            isDeletion = false
        )
        assertTrue("Chinese+emoji should skip glyph animation", anim.skipGlyphAnimation)
    }
}
