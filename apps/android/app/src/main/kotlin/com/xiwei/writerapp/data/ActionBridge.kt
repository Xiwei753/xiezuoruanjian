package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.ActionDescriptor
import com.xiwei.writerapp.model.ActionResult

class ActionBridge(private val nativeBridge: NativeCoreBridge) {

    fun listRegisteredActions(): NativeResult<List<ActionDescriptor>> {
        return nativeBridge.listRegisteredActions()
    }

    fun executeAction(actionId: String, argsJson: String = "{}"): NativeResult<ActionResult> {
        return nativeBridge.executeAction(actionId, argsJson)
    }
}
