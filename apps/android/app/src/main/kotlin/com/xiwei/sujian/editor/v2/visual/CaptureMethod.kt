package com.xiwei.sujian.editor.v2.visual

enum class CaptureMethod {
    PIXEL_COPY,
    SOFTWARE_DRAW
}

enum class PixelCopyResult {
    SUCCESS,
    TIMED_OUT,
    FAILED,
    NOT_SUPPORTED
}
