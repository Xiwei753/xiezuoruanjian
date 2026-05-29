package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.ActionDescriptor
import com.xiwei.writerapp.model.ActionResult

class ActionBridge internal constructor(private val nativeBridge: NativeCoreBridge) {

    fun listRegisteredActions(): BridgeResult<List<ActionDescriptor>> {
        return nativeBridge.listRegisteredActions().toBridgeResult()
    }

    fun executeAction(actionId: String, argsJson: String = "{}"): BridgeResult<ActionResult> {
        return nativeBridge.executeAction(actionId, argsJson).toBridgeResult()
    }
}
