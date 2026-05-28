import re

with open("apps/android/app/src/main/kotlin/com/xiwei/writerapp/ui/StarMapController.kt", "r") as f:
    content = f.read()

new_methods = """
    fun showNewNodeDialog() {
        if (starmapId.isEmpty()) return
        
        val layout = android.widget.LinearLayout(activity)
        layout.orientation = android.widget.LinearLayout.VERTICAL
        layout.setPadding(48, 48, 48, 48)
        
        val titleInput = android.widget.EditText(activity)
        titleInput.hint = "节点名称"
        layout.addView(titleInput)
        
        val kindSpinner = android.widget.Spinner(activity)
        val kinds = arrayOf("角色", "地点", "事件", "物品", "概念", "章节", "其它")
        val kindMap = mapOf(
            "角色" to com.xiwei.writerapp.model.StarMapNodeKind.Character,
            "地点" to com.xiwei.writerapp.model.StarMapNodeKind.Location,
            "事件" to com.xiwei.writerapp.model.StarMapNodeKind.Event,
            "物品" to com.xiwei.writerapp.model.StarMapNodeKind.Item,
            "概念" to com.xiwei.writerapp.model.StarMapNodeKind.Concept,
            "章节" to com.xiwei.writerapp.model.StarMapNodeKind.Chapter,
            "其它" to com.xiwei.writerapp.model.StarMapNodeKind.Custom
        )
        val adapter = android.widget.ArrayAdapter(activity, android.R.layout.simple_spinner_item, kinds)
        adapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item)
        kindSpinner.adapter = adapter
        layout.addView(kindSpinner)
        
        androidx.appcompat.app.AlertDialog.Builder(activity)
            .setTitle("新建节点")
            .setView(layout)
            .setPositiveButton("确定") { _, _ ->
                val title = titleInput.text.toString().trim()
                if (title.isNotEmpty()) {
                    val kindStr = kindSpinner.selectedItem.toString()
                    val kind = kindMap[kindStr] ?: com.xiwei.writerapp.model.StarMapNodeKind.Custom
                    addNode(title, kind)
                }
            }
            .setNegativeButton("取消", null)
            .show()
    }

    private fun addNode(title: String, kind: com.xiwei.writerapp.model.StarMapNodeKind) {
        val node = com.xiwei.writerapp.model.StarMapGraphNode(
            id = java.util.UUID.randomUUID().toString(),
            kind = kind,
            title = title,
            desc = "",
            createdAt = "",
            updatedAt = "",
            color = null,
            icon = null,
            tags = emptyList(),
            attributes = emptyMap()
        )
        CoroutineScope(Dispatchers.IO).launch {
            val result = bridge.addStarmapNode(starmapId, node)
            withContext(Dispatchers.Main) {
                if (result is NativeResult.Success) {
                    loadGraph()
                } else if (result is NativeResult.Error) {
                    Toast.makeText(activity, "创建失败: ${result.message}", Toast.LENGTH_SHORT).show()
                }
            }
        }
    }
}
"""

content = re.sub(r'}\n$', new_methods, content)

with open("apps/android/app/src/main/kotlin/com/xiwei/writerapp/ui/StarMapController.kt", "w") as f:
    f.write(content)

