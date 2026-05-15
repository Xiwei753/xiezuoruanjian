package com.xiwei.writerapp.ui

import android.content.Intent
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
import com.xiwei.writerapp.model.Project
import com.xiwei.writerapp.model.RecentEdit
import com.xiwei.writerapp.model.LocalSettings
import androidx.appcompat.app.AppCompatDelegate

class MainActivity : AppCompatActivity() {
    private lateinit var projectRecyclerView: RecyclerView
    private lateinit var recentEditsRecyclerView: RecyclerView
    private lateinit var fabNewProject: ExtendedFloatingActionButton
    private lateinit var emptyStateLayout: View
    private lateinit var recentEditsLayout: View
    private lateinit var btnSettings: ImageView

    private lateinit var workspaceRepository: WorkspaceRepository
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

        projectRecyclerView = findViewById(R.id.projectRecyclerView)
        recentEditsRecyclerView = findViewById(R.id.recentEditsRecyclerView)
        fabNewProject = findViewById(R.id.fabNewProject)
        emptyStateLayout = findViewById(R.id.emptyStateLayout)
        recentEditsLayout = findViewById(R.id.recentEditsLayout)
        btnSettings = findViewById(R.id.btnSettings)

        ErrorUtil.safeRun(this) {
            workspaceRepository = WorkspaceRepository(this)
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

        btnSettings.setOnClickListener {
            startActivity(Intent(this, SettingsActivity::class.java))
        }
    }

    override fun onResume() {
        super.onResume()
        loadProjects()
        loadRecentEdits()
    }

    private fun loadRecentEdits() {
        recentEdits = ErrorUtil.safeRun(this, emptyList()) {
            if (::workspaceRepository.isInitialized) {
                workspaceRepository.getRecentEdits().take(3) // Only show top 3 on main screen
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

    private fun loadProjects() {
        projects = ErrorUtil.safeRun(this, emptyList()) {
            if (::workspaceRepository.isInitialized) {
                workspaceRepository.getProjects()
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
                    ErrorUtil.safeRun(this) {
                        workspaceRepository.createProject(title)
                        loadProjects()
                    }
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    private inner class ProjectAdapter : RecyclerView.Adapter<ProjectAdapter.ProjectViewHolder>() {

        inner class ProjectViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
            val tvProjectTitle: TextView = itemView.findViewById(R.id.tvProjectTitle)
            val tvProjectDate: TextView = itemView.findViewById(R.id.tvProjectDate)

            init {
                itemView.setOnClickListener {
                    val selectedProject = projects[adapterPosition]
                    val intent = Intent(this@MainActivity, ChapterListActivity::class.java).apply {
                        putExtra("PROJECT_ID", selectedProject.id)
                        putExtra("PROJECT_TITLE", selectedProject.title)
                    }
                    startActivity(intent)
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
