package com.xiwei.writerapp.ui

import android.os.Bundle
import android.view.View
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.button.MaterialButton
import com.google.android.material.card.MaterialCardView
import com.google.android.material.materialswitch.MaterialSwitch
import com.google.android.material.slider.Slider
import com.google.android.material.divider.MaterialDivider
import com.xiwei.writerapp.R
import com.xiwei.writerapp.data.ActionBridge
import com.xiwei.writerapp.data.BridgeResult
import com.xiwei.writerapp.data.BridgeProvider
import com.xiwei.writerapp.model.ActionResult
import com.xiwei.writerapp.model.ActionDescriptor
import com.xiwei.writerapp.model.UiSchemaDescriptor
import com.xiwei.writerapp.model.InputSchemaProperty
import com.google.gson.JsonElement
import com.google.gson.JsonObject

/**
 * ActionRegistryActivity — Action 注册表调试页面
 *
 * 列出所有注册的 Action，支持执行 Query/Mutation 类型操作。
 *
 * ## 架构定位
 * - 调试用途，不面向普通用户
 * - 通过 ActionBridge 调用 Rust Core 的 Action 系统
 *
 * ## 职责边界
 * - **做**：展示 Action 列表、执行 Action、展示结果
 * - **不做**：Action 的注册和定义（由 Rust Core 负责）
 *
 * ## 使用场景
 * - 开发者调试 Action 系统
 * - 测试 Rust Core 暴露的操作接口
 */
class ActionRegistryActivity : AppCompatActivity() {

    private lateinit var bridge: ActionBridge
    private lateinit var actionContainer: LinearLayout
    private lateinit var tvLoading: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_action_registry)

         
        bridge = BridgeProvider.getActionBridge(this)
        actionContainer = findViewById(R.id.actionContainer)
        tvLoading = findViewById(R.id.tvLoading)

        val toolbar: MaterialToolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
        toolbar.setNavigationOnClickListener { finish() }

        loadActions()
    }

    private fun loadActions() {
        tvLoading.visibility = View.VISIBLE
        actionContainer.removeViews(1, actionContainer.childCount - 1)

        val result = bridge.listRegisteredActions()
        when (result) {
            is BridgeResult.Success -> {
                tvLoading.visibility = View.GONE
                renderActions(result.data)
            }
            is BridgeResult.Error -> {
                tvLoading.text = "加载失败: ${result.message}"
            }
            BridgeResult.NotLoaded -> {
                tvLoading.text = getString(R.string.action_not_loaded)
            }
        }
    }

    private fun renderActions(actions: List<ActionDescriptor>) {
        val grouped = actions.groupBy { it.category }

        grouped.forEach { (category, categoryActions) ->
            actionContainer.addView(createCategoryHeader(category))

            categoryActions.forEach { action ->
                actionContainer.addView(createActionCard(action))
                actionContainer.addView(createSpacer())
            }
        }
    }

    private fun createCategoryHeader(category: String): View {
        return TextView(this).apply {
            text = category.uppercase()
            textSize = 12f
            setTextColor(getColor(com.google.android.material.R.color.material_dynamic_primary40))
            setPadding(0, 16, 0, 8)
        }
    }

    private fun createActionCard(action: ActionDescriptor): View {
        val card = MaterialCardView(this).apply {
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
            )
            radius = 16f
            elevation = 2f
        }

        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(16, 16, 16, 16)
        }

        content.addView(TextView(this@ActionRegistryActivity).apply {
            text = action.title
            textSize = 16f
            setTextColor(getColor(com.google.android.material.R.color.material_on_surface_emphasis_high_type))
        })

        content.addView(TextView(this@ActionRegistryActivity).apply {
            text = action.description
            textSize = 14f
            setTextColor(getColor(com.google.android.material.R.color.material_on_surface_emphasis_medium))
            setPadding(0, 4, 0, 8)
        })

        content.addView(TextView(this@ActionRegistryActivity).apply {
            text = "ID: ${action.id}"
            textSize = 12f
            setTextColor(getColor(com.google.android.material.R.color.material_on_surface_emphasis_medium))
        })

        val metaLine = "${kindLabel(action.kind)} | ${riskLabel(action.riskLevel)}"
        content.addView(TextView(this@ActionRegistryActivity).apply {
            text = metaLine
            textSize = 12f
            setPadding(0, 4, 0, 8)
        })

        if (action.confirmRequired) {
            content.addView(TextView(this@ActionRegistryActivity).apply {
                text = "需要确认"
                textSize = 12f
                setTextColor(getColor(com.google.android.material.R.color.material_dynamic_tertiary50))
                setPadding(0, 0, 0, 8)
            })
        }

        val isBlocked = isActionBlocked(action)
        if (isBlocked) {
            content.addView(TextView(this@ActionRegistryActivity).apply {
                text = getString(R.string.action_blocked_dangerous)
                textSize = 12f
                setTextColor(getColor(com.google.android.material.R.color.design_default_color_error))
                setPadding(0, 0, 0, 8)
            })
        }

        val uiContainer = LinearLayout(this@ActionRegistryActivity).apply {
            orientation = LinearLayout.VERTICAL
            id = View.generateViewId()
        }
        content.addView(uiContainer)

        val resultContainer = LinearLayout(this@ActionRegistryActivity).apply {
            orientation = LinearLayout.VERTICAL
            id = View.generateViewId()
            visibility = View.GONE
        }
        content.addView(resultContainer)

        val buttonRow = LinearLayout(this@ActionRegistryActivity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = android.view.Gravity.END
        }

        if (!isBlocked) {
            if (isMutation(action)) {
                val btnApply = MaterialButton(this@ActionRegistryActivity).apply {
                    text = getString(R.string.action_apply)
                    textSize = 14f
                    setOnClickListener {
                        executeMutationAction(action, uiContainer, resultContainer)
                    }
                }
                buttonRow.addView(btnApply)
            } else {
                val btnRun = MaterialButton(this@ActionRegistryActivity).apply {
                    text = getString(R.string.action_run)
                    textSize = 14f
                    setOnClickListener {
                        executeQueryAction(action, resultContainer)
                    }
                }
                buttonRow.addView(btnRun)
            }
        }

        content.addView(buttonRow)
        card.addView(content)

        if (!isBlocked && isMutation(action)) {
            preloadMutationState(action, uiContainer)
        }

        return card
    }

    private fun preloadMutationState(action: ActionDescriptor, uiContainer: LinearLayout) {
        val getActionId = action.id.replace(".set", ".get")
        if (getActionId == action.id) return

        Thread {
            val result = bridge.executeAction(getActionId)
            runOnUiThread {
                if (result is BridgeResult.Success) {
                    val data = result.data.data
                    if (data != null) {
                        populateUiFromData(action, data, uiContainer)
                    }
                }
            }
        }.start()
    }

    private fun populateUiFromData(action: ActionDescriptor, data: JsonElement, uiContainer: LinearLayout) {
        val uiSchema = UiSchemaDescriptor.fromJson(action.uiSchema)
        val inputProps = InputSchemaProperty.fromJson(action.inputSchema)

        uiContainer.removeAllViews()

        when (uiSchema?.type) {
            "slider" -> {
                val prop = inputProps.find { it.type == "number" }
                val currentValue = when (prop?.name) {
                    "fontSize" -> data.asJsonObject.get("fontSize")?.asDouble ?: 16.0
                    "delayMs" -> data.asJsonObject.get("delayMs")?.asDouble ?: 1500.0
                    else -> uiSchema.min ?: 0.0
                }

                val valueLabel = TextView(this).apply {
                    textSize = 14f
                    setPadding(0, 8, 0, 4)
                }

                val slider = Slider(this).apply {
                    valueFrom = (uiSchema.min ?: 0.0).toFloat()
                    valueTo = (uiSchema.max ?: 100.0).toFloat()
                    stepSize = (uiSchema.step ?: 1.0).toFloat()
                    value = currentValue.toFloat()
                    addOnChangeListener { _, value, _ ->
                        valueLabel.text = formatSliderValue(action.id, value.toDouble())
                    }
                }

                valueLabel.text = formatSliderValue(action.id, currentValue)
                uiContainer.addView(valueLabel)
                uiContainer.addView(slider)
            }
            "switch" -> {
                val prop = inputProps.find { it.type == "boolean" }
                val currentValue = when (prop?.name) {
                    "enabled" -> data.asJsonObject.get("enabled")?.asBoolean ?: false
                    else -> false
                }

                val switch = MaterialSwitch(this).apply {
                    text = prop?.name ?: "Enabled"
                    isChecked = currentValue
                    setPadding(0, 8, 0, 8)
                }
                uiContainer.addView(switch)
            }
            else -> {
                if (inputProps.isNotEmpty()) {
                    inputProps.forEach { prop ->
                        val currentValue = data.asJsonObject.get(prop.name)?.asString ?: ""
                        val label = TextView(this).apply {
                            text = "${prop.name}: $currentValue"
                            textSize = 14f
                            setPadding(0, 4, 0, 4)
                        }
                        uiContainer.addView(label)
                    }
                }
            }
        }
    }

    private fun formatSliderValue(actionId: String, value: Double): String {
        return when {
            actionId.contains("font_size") -> "${value.toInt()}sp"
            actionId.contains("auto_save_delay") -> "${(value / 1000).toInt()}s"
            else -> value.toString()
        }
    }

    private fun executeQueryAction(action: ActionDescriptor, resultContainer: LinearLayout) {
        resultContainer.removeAllViews()
        resultContainer.visibility = View.VISIBLE

        Thread {
            val result = bridge.executeAction(action.id)
            runOnUiThread {
                showActionResult(result, action, resultContainer)
            }
        }.start()
    }

    private fun executeMutationAction(
        action: ActionDescriptor,
        uiContainer: LinearLayout,
        resultContainer: LinearLayout
    ) {
        if (action.confirmRequired) {
            AlertDialog.Builder(this)
                .setTitle(R.string.action_confirm_title)
                .setMessage(R.string.action_confirm_message)
                .setPositiveButton(R.string.action_ok) { _, _ ->
                    doExecuteMutation(action, uiContainer, resultContainer)
                }
                .setNegativeButton(R.string.action_cancel, null)
                .show()
        } else {
            doExecuteMutation(action, uiContainer, resultContainer)
        }
    }

    private fun doExecuteMutation(
        action: ActionDescriptor,
        uiContainer: LinearLayout,
        resultContainer: LinearLayout
    ) {
        resultContainer.removeAllViews()
        resultContainer.visibility = View.VISIBLE

        val argsJson = buildArgsJson(action, uiContainer)

        Thread {
            val result = bridge.executeAction(action.id, argsJson)
            runOnUiThread {
                showActionResult(result, action, resultContainer)
            }
        }.start()
    }

    private fun buildArgsJson(action: ActionDescriptor, uiContainer: LinearLayout): String {
        val uiSchema = UiSchemaDescriptor.fromJson(action.uiSchema)
        val inputProps = InputSchemaProperty.fromJson(action.inputSchema)

        return when (uiSchema?.type) {
            "slider" -> {
                val slider = findChildByType(uiContainer, Slider::class.java)
                val prop = inputProps.find { it.type == "number" }
                val value = slider?.value?.toDouble() ?: 0.0
                if (prop != null) {
                    JsonObject().apply { addProperty(prop.name, value) }.toString()
                } else {
                    JsonObject().apply { addProperty("value", value) }.toString()
                }
            }
            "switch" -> {
                val switch = findChildByType(uiContainer, MaterialSwitch::class.java)
                val prop = inputProps.find { it.type == "boolean" }
                val checked = switch?.isChecked ?: false
                if (prop != null) {
                    JsonObject().apply { addProperty(prop.name, checked) }.toString()
                } else {
                    JsonObject().apply { addProperty("enabled", checked) }.toString()
                }
            }
            else -> {
                "{}"
            }
        }
    }

    private fun <T : View> findChildByType(parent: LinearLayout, clazz: Class<T>): T? {
        for (i in 0 until parent.childCount) {
            val child = parent.getChildAt(i)
            if (clazz.isInstance(child)) {
                @Suppress("UNCHECKED_CAST")
                return child as T
            }
        }
        return null
    }

    private fun showActionResult(
        result: BridgeResult<ActionResult>,
        action: ActionDescriptor,
        resultContainer: LinearLayout
    ) {
        resultContainer.removeAllViews()

        when (result) {
            is BridgeResult.Success -> {
                val actionResult = result.data
                val msg = actionResult.message ?: if (actionResult.success) "执行成功" else "执行失败"

                resultContainer.addView(TextView(this).apply {
                    text = msg
                    textSize = 14f
                    setTextColor(if (actionResult.success)
                        getColor(com.google.android.material.R.color.material_dynamic_primary50)
                    else
                        getColor(com.google.android.material.R.color.design_default_color_error))
                    setPadding(0, 8, 0, 4)
                })

                if (actionResult.data != null) {
                    val dataStr = actionResult.data.toString()
                    resultContainer.addView(TextView(this).apply {
                        text = "返回数据:\n$dataStr"
                        textSize = 12f
                        setTextColor(getColor(com.google.android.material.R.color.material_on_surface_emphasis_medium))
                        setPadding(0, 4, 0, 0)
                    })
                }
            }
            is BridgeResult.Error -> {
                resultContainer.addView(TextView(this).apply {
                    text = "错误: ${result.message}"
                    textSize = 14f
                    setTextColor(getColor(com.google.android.material.R.color.design_default_color_error))
                    setPadding(0, 8, 0, 0)
                })
            }
            BridgeResult.NotLoaded -> {
                resultContainer.addView(TextView(this).apply {
                    text = getString(R.string.action_not_loaded)
                    textSize = 14f
                    setPadding(0, 8, 0, 0)
                })
            }
        }
    }

    private fun normalizedKind(action: ActionDescriptor): String {
        return action.kind.lowercase()
    }

    private fun normalizedRisk(action: ActionDescriptor): String {
        return action.riskLevel.lowercase()
    }

    private fun isMutation(action: ActionDescriptor): Boolean {
        return normalizedKind(action) == "mutation"
    }

    private fun isBlockedRisk(action: ActionDescriptor): Boolean {
        val risk = normalizedRisk(action)
        return risk == "dangerous" || risk == "contentwrite"
    }

    private fun isActionBlocked(action: ActionDescriptor): Boolean {
        return isBlockedRisk(action)
    }

    private fun kindLabel(kind: String): String {
        return when (kind.lowercase()) {
            "query" -> getString(R.string.action_kind_query)
            "preview" -> getString(R.string.action_kind_preview)
            "mutation" -> getString(R.string.action_kind_mutation)
            else -> kind
        }
    }

    private fun riskLabel(risk: String): String {
        return when (risk.lowercase()) {
            "saferead" -> getString(R.string.risk_safe_read)
            "safewrite" -> getString(R.string.risk_safe_write)
            "contentwrite" -> getString(R.string.risk_content_write)
            "dangerous" -> getString(R.string.risk_dangerous)
            else -> risk
        }
    }

    private fun createSpacer(): View {
        return View(this).apply {
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                8
            )
        }
    }
}
