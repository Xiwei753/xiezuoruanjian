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
import androidx.appcompat.app.AppCompatDelegate

class MainActivity : AppCompatActivity() {
    private lateinit var projectRecyclerView: RecyclerView
    private lateinit var fabNewProject: ExtendedFloatingActionButton
    private lateinit var emptyStateLayout: View
    private lateinit var btnSettings: ImageView

    private lateinit var workspaceRepository: WorkspaceRepository
    private lateinit var settingsRepository: SettingsRepository
    private var projects = listOf<Project>()
    private lateinit var adapter: ProjectAdapter

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        try {
            settingsRepository = SettingsRepository(this)
            val settings = settingsRepository.getLocalSettings()
            when (settings.themeMode) {
                "light" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_NO)
                "dark" -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_YES)
                else -> AppCompatDelegate.setDefaultNightMode(AppCompatDelegate.MODE_NIGHT_FOLLOW_SYSTEM)
            }
        } catch (e: Throwable) {
            e.printStackTrace()
        }

        setContentView(R.layout.activity_main)

        projectRecyclerView = findViewById(R.id.projectRecyclerView)
        fabNewProject = findViewById(R.id.fabNewProject)
        emptyStateLayout = findViewById(R.id.emptyStateLayout)
        btnSettings = findViewById(R.id.btnSettings)

        try {
            workspaceRepository = WorkspaceRepository(this)
        } catch (e: Throwable) {
            e.printStackTrace()
            // Even if WorkspaceRepository fails, we continue to prevent crash, loadProjects will handle the error
        }

        adapter = ProjectAdapter()
        projectRecyclerView.layoutManager = LinearLayoutManager(this)
        projectRecyclerView.adapter = adapter

        if (this::workspaceRepository.isInitialized) {
            loadProjects()
        } else {
            projectRecyclerView.visibility = View.GONE
            emptyStateLayout.visibility = View.VISIBLE
        }

        fabNewProject.setOnClickListener {
            if (this::workspaceRepository.isInitialized) {
                showNewProjectDialog()
            } else {
                android.widget.Toast.makeText(this, "作品加载失败，无法创建新作品", android.widget.Toast.LENGTH_LONG).show()
            }
        }

        btnSettings.setOnClickListener {
            startActivity(Intent(this, SettingsActivity::class.java))
        }
    }

    override fun onResume() {
        super.onResume()
        loadProjects()
    }

    private fun loadProjects() {
        projects = ErrorUtil.safeRun(this, emptyList()) {
            workspaceRepository.getProjects()
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
}
