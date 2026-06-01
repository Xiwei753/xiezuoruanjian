package com.xiwei.writerapp.data

import com.google.gson.Gson
import com.google.gson.JsonElement
import com.xiwei.writerapp.model.ActionDescriptor
import com.xiwei.writerapp.model.ActionResult
import uniffi.writer_core.ActionDescriptorDto
import uniffi.writer_core.ActionResultDto

class ActionBridge internal constructor(private val appService: AppServiceBridge) {

    private val gson = Gson()

    fun listRegisteredActions(): BridgeResult<List<ActionDescriptor>> {
        return when (val result = appService.listRegisteredActions()) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.map { it.toModel() })
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }

    fun executeAction(actionId: String, argsJson: String = "{}"): BridgeResult<ActionResult> {
        return when (val result = appService.executeAction(actionId, argsJson, "{}")) {
            is BridgeResult.Success -> BridgeResult.Success(result.data.toModel())
            is BridgeResult.Error -> BridgeResult.Error(result.envelope)
            BridgeResult.NotLoaded -> BridgeResult.NotLoaded
        }
    }
}

private fun ActionDescriptorDto.toModel(): ActionDescriptor = ActionDescriptor(
    id = id,
    title = title,
    description = description,
    category = category,
    kind = kind.name,
    riskLevel = riskLevel.name,
    confirmRequired = confirmRequired,
    undoable = undoable,
    platforms = platforms,
    inputSchema = inputSchema?.let { parseJsonElement(it) },
    uiSchema = uiSchema?.let { parseJsonElement(it) }
)

private fun ActionResultDto.toModel(): ActionResult = ActionResult(
    success = success,
    message = message,
    data = data?.let { parseJsonElement(it) },
    proposedUi = proposedUi?.let { parseJsonElement(it) },
    requiresConfirmation = requiresConfirmation
)

private fun parseJsonElement(json: String): JsonElement? {
    return try {
        com.google.gson.JsonParser.parseString(json)
    } catch (_: Exception) {
        null
    }
}
