package com.xiwei.writerapp.ui

import android.content.Intent

import android.widget.Toast
import android.widget.FrameLayout
import com.google.android.material.appbar.MaterialToolbar
import com.google.android.material.bottomnavigation.BottomNavigationView
import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.ImageView
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.floatingactionbutton.ExtendedFloatingActionButton
import com.xiwei.writerapp.R
import com.xiwei.writerapp.data.SettingsRepository
import com.xiwei.writerapp.data.WorkspaceRepository
import com.xiwei.writerapp.data.WorkspaceUseCase
import com.xiwei.writerapp.model.Project
import com.xiwei.writerapp.model.RecentEdit
import com.xiwei.writerapp.model.LocalSettings
import androidx.appcompat.app.AppCompatDelegate
import android.os.Build
import android.util.Log
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * MainActivity — 应用主界面
 *
 * 包含作品列表、最近编辑、星图和统计四个标签页，是应用的导航中心。
 *
 * ## 架构定位
 * - 应用入口 Activity，管理底部导航和标签页切换
 * - 通过 Repository/领域 Bridge 与 Rust Core 交互
 *
 * ## 职责边界
 * - **做**：作品列表展示、新建作品、标签页切换、主题管理
 * - **不做**：具体业务逻辑（由各 Controller 负责）
 *
 * ## 依赖关系
 * - 领域 Bridge：Rust Core JNI 桥接（legacy adapter 仅在 data 层内部）
 * - StarMapController：星图标签页控制器
 * - StatsController：统计标签页控制器
 *
 * ## 使用场景
 * - 应用启动后的主界面
 * - 作品的创建、浏览和管理
 */
class MainActivity : AppCompatActivity() {
    private lateinit var projectRecyclerView: RecyclerView
    private lateinit var recentEditsRecyclerView: RecyclerView
    private lateinit var fabNewProject: ExtendedFloatingActionButton
    private lateinit var fabNewStarMapNode: ExtendedFloatingActionButton
    private lateinit var emptyStateLayout: View
    private lateinit var recentEditsLayout: View
    private lateinit var btnSettings: ImageView
    private lateinit var tabWorks: FrameLayout
    private lateinit var tabStarMap: FrameLayout
    private lateinit var tabStats: FrameLayout
    private lateinit var bottomNav: BottomNavigationView
    private lateinit var canvasView: StarMapCanvasView
    private lateinit var toolbar: MaterialToolbar

    var starmapId: String = ""
    
    private lateinit var starMapController: StarMapController
    private lateinit var statsController: StatsController

    private lateinit var workspaceRepository: WorkspaceRepository
    private lateinit var workspaceUseCase: WorkspaceUseCase
    private lateinit var settingsRepository: SettingsRepository
    private var projects = listOf<Project>()
    private var recentEdits = listOf<RecentEdit>()
    private lateinit var adapter: ProjectAdapter
    private lateinit var recentAdapter: RecentEditAdapter

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        ErrorUtil.safeRun(this) {
            settingsRepository = SettingsRepository(this)
            val settings = ErrorUtil.safeRun(this, LocalSettings()) {
                settingsRepository.getLocalSettings()
            }
            when (settings.themeMode) {
                "light" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_NO)
                "dark" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_YES)
                else -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM)
            }
        }

        setContentView(R.layout.activity_main)

        window.decorView.post {
            UiFontUtil.applySansSerifFallback(window.decorView.rootView)
        }

        projectRecyclerView = findViewById(R.id.projectRecyclerView)
        recentEditsRecyclerView = findViewById(R.id.recentEditsRecyclerView)
        fabNewProject = findViewById(R.id.fabNewProject)
        fabNewStarMapNode = findViewById(R.id.fabNewStarMapNode)
        emptyStateLayout = findViewById(R.id.emptyStateLayout)
        recentEditsLayout = findViewById(R.id.recentEditsLayout)
        btnSettings = findViewById(R.id.btnSettings)
        tabWorks = findViewById(R.id.tabWorks)
        tabStarMap = findViewById(R.id.tabStarMap)
        tabStats = findViewById(R.id.tabStats)
        bottomNav = findViewById(R.id.bottomNav)
        canvasView = findViewById(R.id.canvasView)
        toolbar = findViewById(R.id.toolbar)

        starMapController = StarMapController(this, com.xiwei.writerapp.data.BridgeProvider.getStarmapBridge(this), tabStarMap, canvasView)
        statsController = StatsController(this, com.xiwei.writerapp.data.BridgeProvider.getStatsBridge(this), tabStats)

        // Sync initial state
        when (bottomNav.selectedItemId) {
            R.id.nav_works -> {
                tabWorks.visibility = View.VISIBLE
                tabStarMap.visibility = View.GONE
                tabStats.visibility = View.GONE
                toolbar.title = "作品"
                fabNewProject.show()
                fabNewStarMapNode.hide()
            }
            R.id.nav_starmap -> {
                tabWorks.visibility = View.GONE
                tabStarMap.visibility = View.VISIBLE
                tabStats.visibility = View.GONE
                toolbar.title = "星图"
                starMapController.initialize(starmapId)
                fabNewProject.hide()
                fabNewStarMapNode.show()
            }
            R.id.nav_stats -> {
                tabWorks.visibility = View.GONE
                tabStarMap.visibility = View.GONE
                tabStats.visibility = View.VISIBLE
                toolbar.title = "统计"
                statsController.initialize()
                fabNewProject.hide()
                fabNewStarMapNode.hide()
            }
        }

        bottomNav.setOnItemSelectedListener { item ->
            when (item.itemId) {
                R.id.nav_works -> {
                    tabWorks.visibility = View.VISIBLE
                    tabStarMap.visibility = View.GONE
                    tabStats.visibility = View.GONE
                    toolbar.title = "作品"
                    fabNewProject.show()
                    fabNewStarMapNode.hide()
                    true
                }
                R.id.nav_starmap -> {
                    tabWorks.visibility = View.GONE
                    tabStarMap.visibility = View.VISIBLE
                    tabStats.visibility = View.GONE
                    toolbar.title = "星图"
                    starMapController.initialize(starmapId)
                    fabNewProject.hide()
                    fabNewStarMapNode.show()
                    true
                }
                R.id.nav_stats -> {
                    tabWorks.visibility = View.GONE
                    tabStarMap.visibility = View.GONE
                    tabStats.visibility = View.VISIBLE
                    toolbar.title = "统计"
                    statsController.initialize()
                    fabNewProject.hide()
                    fabNewStarMapNode.hide()
                    true
                }
                else -> false
            }
        }

        ErrorUtil.safeRun(this) {
            workspaceRepository = WorkspaceRepository(this)
            workspaceUseCase = WorkspaceUseCase(workspaceRepository)
        }

        adapter = ProjectAdapter()
        projectRecyclerView.layoutManager = LinearLayoutManager(this)
        projectRecyclerView.adapter = adapter

        recentAdapter = RecentEditAdapter()
        recentEditsRecyclerView.layoutManager = LinearLayoutManager(this)
        recentEditsRecyclerView.adapter = recentAdapter

        loadProjects()
        loadRecentEdits()

        fabNewProject.setOnClickListener {
            showNewProjectDialog()
        }

        fabNewStarMapNode.setOnClickListener {
            starMapController.showNewNodeDialog()
        }



        btnSettings.setOnClickListener {
            startActivity(Intent(this, SettingsActivity::class.java))
        }
    }


    fun onStarmapIdInitialized(id: String) {
        this.starmapId = id
    }

    override fun onResume() {
        super.onResume()
        loadProjects()
        loadRecentEdits()
        syncMonetColor()
    }

    private fun syncMonetColor() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            lifecycleScope.launch {
                try {
                    val colorInt = resources.getColor(android.R.color.system_accent1_500, theme)
                    val hexColor = String.format("#%06X", 0xFFFFFF and colorInt)

                    if (::settingsRepository.isInitialized) {
                        withContext(Dispatchers.IO) {
                            val syncable = settingsRepository.getSyncableSettings()
                            if (syncable.monetColor != hexColor) {
                                settingsRepository.saveSyncableSettings(syncable.copy(monetColor = hexColor))
                            }
                        }
                    }
                } catch (e: Exception) {
                    Log.w("MainActivity", "Failed to extract Monet color", e)
                }
            }
        }
    }

    private fun loadRecentEdits() {
        lifecycleScope.launch {
            recentEdits = ErrorUtil.safeRunSuspend(this@MainActivity, emptyList()) {
                if (::workspaceUseCase.isInitialized) {
                    workspaceUseCase.getRecentEdits(3)
                } else {
                    emptyList()
                }
            }

            if (recentEdits.isEmpty()) {
                recentEditsLayout.visibility = View.GONE
            } else {
                recentEditsLayout.visibility = View.VISIBLE
                recentAdapter.notifyDataSetChanged()
            }
        }
    }

    private fun loadProjects() {
        lifecycleScope.launch {
            projects = ErrorUtil.safeRunSuspend(this@MainActivity, emptyList()) {
                if (::workspaceUseCase.isInitialized) {
                    workspaceUseCase.getProjects()
                } else {
                    emptyList()
                }
            }

            if (projects.isEmpty()) {
                projectRecyclerView.visibility = View.GONE
                emptyStateLayout.visibility = View.VISIBLE
            } else {
                projectRecyclerView.visibility = View.VISIBLE
                emptyStateLayout.visibility = View.GONE
                adapter.notifyDataSetChanged()
            }
        }
    }

    private fun showNewProjectDialog() {
        val editText = EditText(this)
        editText.hint = getString(R.string.hint_project_title)
        editText.setPadding(48, 48, 48, 48)

        AlertDialog.Builder(this)
            .setTitle(R.string.dialog_new_project_title)
            .setView(editText)
            .setPositiveButton(R.string.action_create) { _, _ ->
                val title = editText.text.toString().trim()
                if (title.isNotEmpty()) {
                    lifecycleScope.launch {
                        ErrorUtil.safeRunSuspend(this@MainActivity) {
                            workspaceUseCase.createProject(title)
                        }
                        loadProjects()
                    }
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }


    private fun showProjectMenu(view: View, project: Project, position: Int) {
        val popup = android.widget.PopupMenu(this, view)
        popup.menu.add(0, 1, 0, getString(R.string.action_rename))
        popup.menu.add(0, 2, 0, getString(R.string.action_delete))
        if (position > 0) {
            popup.menu.add(0, 3, 0, getString(R.string.action_move_up))
        }
        if (position < projects.size - 1) {
            popup.menu.add(0, 4, 0, getString(R.string.action_move_down))
        }

        popup.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                1 -> {
                    showRenameProjectDialog(project)
                    true
                }
                2 -> {
                    showDeleteProjectDialog(project)
                    true
                }
                3 -> {
                    moveProjectUp(position)
                    true
                }
                4 -> {
                    moveProjectDown(position)
                    true
                }
                else -> false
            }
        }
        popup.show()
    }

    private fun showRenameProjectDialog(project: Project) {
        val editText = android.widget.EditText(this)
        editText.setText(project.title)
        editText.setSelection(project.title.length)
        editText.hint = getString(R.string.hint_project_title)
        editText.setPadding(48, 48, 48, 48)

        AlertDialog.Builder(this)
            .setTitle(R.string.action_rename)
            .setView(editText)
            .setPositiveButton(R.string.action_ok) { _, _ ->
                val newTitle = editText.text.toString().trim()
                if (newTitle.isNotEmpty() && newTitle != project.title) {
                    lifecycleScope.launch {
                        ErrorUtil.safeRunSuspend(this@MainActivity) {
                            workspaceUseCase.renameProject(project.id, newTitle)
                        }
                        loadProjects()
                    }
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    private fun showDeleteProjectDialog(project: Project) {
        AlertDialog.Builder(this)
            .setTitle(R.string.confirm_delete_project)
            .setMessage("确定要删除作品 \"${project.title}\" 吗？此操作无法恢复。")
            .setPositiveButton(R.string.action_delete) { _, _ ->
                lifecycleScope.launch {
                    ErrorUtil.safeRunSuspend(this@MainActivity) {
                        workspaceUseCase.deleteProject(project.id)
                    }
                    loadProjects()
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    private fun moveProjectUp(position: Int) {
        if (position <= 0) return
        val orderedIds = projects.map { it.id }.toMutableList()
        val temp = orderedIds[position]
        orderedIds[position] = orderedIds[position - 1]
        orderedIds[position - 1] = temp

        lifecycleScope.launch {
            ErrorUtil.safeRunSuspend(this@MainActivity) {
                workspaceUseCase.reorderProjects(orderedIds)
            }
            loadProjects()
        }
    }

    private fun moveProjectDown(position: Int) {
        if (position >= projects.size - 1) return
        val orderedIds = projects.map { it.id }.toMutableList()
        val temp = orderedIds[position]
        orderedIds[position] = orderedIds[position + 1]
        orderedIds[position + 1] = temp

        lifecycleScope.launch {
            ErrorUtil.safeRunSuspend(this@MainActivity) {
                workspaceUseCase.reorderProjects(orderedIds)
            }
            loadProjects()
        }
    }

    private inner class ProjectAdapter : RecyclerView.Adapter<ProjectAdapter.ProjectViewHolder>() {

        inner class ProjectViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
            val tvProjectTitle: TextView = itemView.findViewById(R.id.tvProjectTitle)
            val tvProjectDate: TextView = itemView.findViewById(R.id.tvProjectDate)
            val btnMoreProject: android.widget.ImageButton = itemView.findViewById(R.id.btnMoreProject)

            init {
                itemView.isHapticFeedbackEnabled = false
                btnMoreProject.isHapticFeedbackEnabled = false

                btnMoreProject.setOnClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val project = projects[pos]
                        showProjectMenu(btnMoreProject, project, pos)
                    }
                }

                itemView.setOnClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val selectedProject = projects[pos]
                        val intent = Intent(this@MainActivity, ChapterListActivity::class.java).apply {
                            putExtra("PROJECT_ID", selectedProject.id)
                            putExtra("PROJECT_TITLE", selectedProject.title)
                        }
                        startActivity(intent)
                    }
                }

                itemView.setOnLongClickListener {
                    val pos = adapterPosition
                    if (pos != RecyclerView.NO_POSITION) {
                        val project = projects[pos]
                        showProjectMenu(itemView, project, pos)
                    }
                    true
                }
            }
        }

        override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ProjectViewHolder {
            val view = LayoutInflater.from(parent.context).inflate(R.layout.item_project, parent, false)
            return ProjectViewHolder(view)
        }

        override fun onBindViewHolder(holder: ProjectViewHolder, position: Int) {
            val project = projects[position]
            holder.tvProjectTitle.text = project.title
            // Simplified date display for MVP
            holder.tvProjectDate.text = project.updatedAt.substringBefore("T")
        }

        override fun getItemCount() = projects.size
    }

    private inner class RecentEditAdapter : RecyclerView.Adapter<RecentEditAdapter.RecentViewHolder>() {

        inner class RecentViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
            val tvRecentTitle: TextView = itemView.findViewById(R.id.tvRecentTitle)
            val tvRecentSubtitle: TextView = itemView.findViewById(R.id.tvRecentSubtitle)

            init {
                itemView.setOnClickListener {
                    val edit = recentEdits[adapterPosition]
                    val intent = Intent(this@MainActivity, EditorActivity::class.java).apply {
                        putExtra("PROJECT_ID", edit.projectId)
                        putExtra("VOLUME_ID", edit.volumeId)
                        putExtra("CHAPTER_ID", edit.chapterId)
                        // Note: chapterTitle is missing here, we'll gracefully fallback in EditorActivity
                    }
                    startActivity(intent)
                }
            }
        }

        override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): RecentViewHolder {
            val view = LayoutInflater.from(parent.context).inflate(R.layout.item_recent_edit, parent, false)
            return RecentViewHolder(view)
        }

        override fun onBindViewHolder(holder: RecentViewHolder, position: Int) {
            val edit = recentEdits[position]
            // We just show the IDs if we don't have the titles handy,
            // but ideally we should fetch the titles. For MVP, we'll try to find the project title.
            val project = projects.find { it.id == edit.projectId }
            holder.tvRecentTitle.text = project?.title ?: "未知作品"
            holder.tvRecentSubtitle.text = "继续编写..."
        }

        override fun getItemCount() = recentEdits.size
    }
}
