package com.xiwei.sujian.editor.v2.host

import com.xiwei.sujian.data.AppServiceBridge
import com.xiwei.sujian.data.BridgeResult
import com.xiwei.sujian.editor.v2.mirror.EditResult

class UniFFIEditorKernelBridge(
    private val appServiceBridge: AppServiceBridge
) : EditorKernelBridge {

    override fun apply(commandJson: String): String {
        return when (val result = appServiceBridge.editorKernelApply(commandJson)) {
            is BridgeResult.Success -> {
                val dto = result.data
                com.xiwei.sujian.editor.v2.mirror.EditResultDtoMapper.dtoToJson(dto)
            }
            else -> "{}"
        }
    }

    override fun loadText(text: String, cursorUtf8: Int): String {
        return when (val result = appServiceBridge.editorKernelLoadText(text, cursorUtf8.toUInt())) {
            is BridgeResult.Success -> {
                val dto = result.data
                com.xiwei.sujian.editor.v2.mirror.EditResultDtoMapper.dtoToJson(dto)
            }
            else -> "{}"
        }
    }
}
