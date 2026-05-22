package com.xiwei.writerapp.ui.mindmap

import android.os.Bundle
import android.view.Menu
import android.view.MenuItem
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.appcompat.widget.Toolbar
import com.xiwei.writerapp.R
import com.xiwei.writerapp.data.NativeCoreBridge
import com.xiwei.writerapp.data.NativeResult
import com.xiwei.writerapp.data.WorkspaceManager
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MindMapActivity : AppCompatActivity() {

    private lateinit var mindMapRenderView: MindMapRenderView
    private lateinit var toolbar: Toolbar
    private var projectId: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_mind_map)

        toolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
        supportActionBar?.setDisplayHomeAsUpEnabled(true)

        mindMapRenderView = findViewById(R.id.mindMapRenderView)

        projectId = intent.getStringExtra("EXTRA_PROJECT_ID")
        val projectTitle = intent.getStringExtra("EXTRA_PROJECT_TITLE") ?: "导图"

        supportActionBar?.title = projectTitle

        if (projectId.isNullOrEmpty()) {
            Toast.makeText(this, "未找到项目 ID", Toast.LENGTH_SHORT).show()
            finish()
            return
        }

        loadSnapshot()
    }

    private fun loadSnapshot() {
        val pid = projectId ?: return
        val workspaceDir = WorkspaceManager.getWorkspaceDir(this)
        if (!workspaceDir.exists()) {
            Toast.makeText(this, "工作区未初始化", Toast.LENGTH_SHORT).show()
            return
        }

        CoroutineScope(Dispatchers.Main).launch {
            val bridge = NativeCoreBridge(this@MindMapActivity)
            val result = withContext(Dispatchers.IO) {
                bridge.getMindMapSnapshot(pid)
            }

            when (result) {
                is NativeResult.Success -> {
                    mindMapRenderView.setSnapshot(result.data)
                }
                is NativeResult.Error -> {
                    Toast.makeText(this@MindMapActivity, "加载导图失败: ${result.message}", Toast.LENGTH_LONG).show()
                }
                is NativeResult.NotLoaded -> {
                    Toast.makeText(this@MindMapActivity, "底层核心未加载", Toast.LENGTH_SHORT).show()
                }
            }
        }
    }

    override fun onCreateOptionsMenu(menu: Menu): Boolean {
        menuInflater.inflate(R.menu.menu_mind_map, menu)
        return true
    }

    override fun onOptionsItemSelected(item: MenuItem): Boolean {
        when (item.itemId) {
            android.R.id.home -> {
                finish()
                return true
            }
            R.id.action_fit_screen -> {
                mindMapRenderView.fitToScreen()
                return true
            }
            R.id.action_relayout -> {
                loadSnapshot()
                return true
            }
            R.id.action_toggle_hud -> {
                mindMapRenderView.toggleHud()
                return true
            }
        }
        return super.onOptionsItemSelected(item)
    }
}
