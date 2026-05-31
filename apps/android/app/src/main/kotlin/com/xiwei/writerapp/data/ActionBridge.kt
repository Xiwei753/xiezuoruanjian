package com.xiwei.writerapp.data

import com.google.gson.Gson
import com.google.gson.reflect.TypeToken
import com.xiwei.writerapp.model.ActionDescriptor
import com.xiwei.writerapp.model.ActionResult

class ActionBridge internal constructor(private val appService: AppServiceBridge) {

    private val gson = Gson()

    fun listRegisteredActions(): BridgeResult<List<ActionDescriptor>> {
        return when (val result = appService.listRegisteredActions()) {
            is BridgeResult.Success -> {
                try {
                    val type = object : TypeToken<List<ActionDescriptor>>() {}.type
                    val actions: List<ActionDescriptor> = gson.fromJson(result.data, type)
                    BridgeResult.Success(actions)
                } catch (e: Exception) {
                    BridgeResult.Error(ResultEnvelope.error("JSON_ERROR", "Failed to parse actions: ${e.message}"))
                }
            }
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> result
        }
    }

    fun executeAction(actionId: String, argsJson: String = "{}"): BridgeResult<ActionResult> {
        return when (val result = appService.executeAction(actionId, argsJson, "{}")) {
            is BridgeResult.Success -> {
                try {
                    val actionResult: ActionResult = gson.fromJson(result.data, ActionResult::class.java)
                    BridgeResult.Success(actionResult)
                } catch (e: Exception) {
                    BridgeResult.Error(ResultEnvelope.error("JSON_ERROR", "Failed to parse action result: ${e.message}"))
                }
            }
            is BridgeResult.Error -> result
            BridgeResult.NotLoaded -> result
        }
    }
}
