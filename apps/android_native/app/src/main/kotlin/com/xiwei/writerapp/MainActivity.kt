package com.xiwei.writerapp

import android.content.Intent
import android.os.Bundle
import android.widget.ArrayAdapter
import android.widget.ListView
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity() {
    private lateinit var projectListView: ListView
    private lateinit var workspaceReader: SampleWorkspaceReader

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        projectListView = findViewById(R.id.projectListView)
        workspaceReader = SampleWorkspaceReader(this)

        val projects = workspaceReader.getProjects()
        val projectTitles = projects.map { it.title }

        val adapter = ArrayAdapter(this, R.layout.item_list, R.id.text1, projectTitles)
        projectListView.adapter = adapter

        projectListView.setOnItemClickListener { _, _, position, _ ->
            val selectedProject = projects[position]
            val intent = Intent(this, ChapterListActivity::class.java).apply {
                putExtra("PROJECT_ID", selectedProject.id)
            }
            startActivity(intent)
        }
    }
}
