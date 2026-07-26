package com.xiwei.sujian.editor.v2.visual

object ColorDistance {

    const val BACKGROUND_TOLERANCE = 48

    fun isClose(pixel: Int, background: Int): Boolean {
        val dr = red(pixel) - red(background)
        val dg = green(pixel) - green(background)
        val db = blue(pixel) - blue(background)
        return dr * dr + dg * dg + db * db <= BACKGROUND_TOLERANCE * BACKGROUND_TOLERANCE
    }

    fun red(color: Int): Int = (color shr 16) and 0xFF

    fun green(color: Int): Int = (color shr 8) and 0xFF

    fun blue(color: Int): Int = color and 0xFF

    fun alpha(color: Int): Int = (color shr 24) and 0xFF

    fun rgb(r: Int, g: Int, b: Int): Int = (0xFF shl 24) or (r shl 16) or (g shl 8) or b
}
