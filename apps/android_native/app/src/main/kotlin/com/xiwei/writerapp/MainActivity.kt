package com.xiwei.writerapp

import android.content.Intent
import android.os.Bundle
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.ListView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity() {
    private lateinit var projectListView: ListView
    private lateinit var btnNewProject: Button
    private lateinit var workspaceRepository: WorkspaceRepository
    private var projects = listOf<Project>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        projectListView = findViewById(R.id.projectListView)
        btnNewProject = findViewById(R.id.btnNewProject)
        workspaceRepository = WorkspaceRepository(this)

        loadProjects()

        projectListView.setOnItemClickListener { _, _, position, _ ->
            val selectedProject = projects[position]
            val intent = Intent(this, ChapterListActivity::class.java).apply {
                putExtra("PROJECT_ID", selectedProject.id)
            }
            startActivity(intent)
        }

        btnNewProject.setOnClickListener {
            showNewProjectDialog()
        }
    }

    override fun onResume() {
        super.onResume()
        loadProjects()
    }

    private fun loadProjects() {
        projects = workspaceRepository.getProjects()
        val projectTitles = projects.map { it.title }

        val adapter = ArrayAdapter(this, R.layout.item_list, R.id.text1, projectTitles)
        projectListView.adapter = adapter
    }

    private fun showNewProjectDialog() {
        val editText = EditText(this)
        editText.hint = "Project Title"

        AlertDialog.Builder(this)
            .setTitle("New Project")
            .setView(editText)
            .setPositiveButton("Create") { _, _ ->
                val title = editText.text.toString().trim()
                if (title.isNotEmpty()) {
                    workspaceRepository.createProject(title)
                    loadProjects()
                }
            }
            .setNegativeButton("Cancel", null)
            .show()
    }
}
