package com.xiwei.writerapp

import android.content.Intent
import android.os.Bundle
import android.widget.ArrayAdapter
import android.widget.Button
import android.widget.EditText
import android.widget.ListView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity

class ChapterListActivity : AppCompatActivity() {
    private lateinit var chapterListView: ListView
    private lateinit var btnNewChapter: Button
    private lateinit var workspaceRepository: WorkspaceRepository
    private var projectId: String? = null
    private var listItems = mutableListOf<ChapterListItem>()

    // Simple class to store combined volume + chapter data for the list
    private data class ChapterListItem(val volumeId: String, val chapterId: String, val displayText: String)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_chapter_list)

        chapterListView = findViewById(R.id.chapterListView)
        btnNewChapter = findViewById(R.id.btnNewChapter)
        workspaceRepository = WorkspaceRepository(this)

        projectId = intent.getStringExtra("PROJECT_ID")
        if (projectId == null) {
            finish()
            return
        }

        loadChapters()

        chapterListView.setOnItemClickListener { _, _, position, _ ->
            val selectedItem = listItems[position]
            val intent = Intent(this, EditorActivity::class.java).apply {
                putExtra("PROJECT_ID", projectId)
                putExtra("VOLUME_ID", selectedItem.volumeId)
                putExtra("CHAPTER_ID", selectedItem.chapterId)
            }
            startActivity(intent)
        }

        btnNewChapter.setOnClickListener {
            showNewChapterDialog()
        }
    }

    override fun onResume() {
        super.onResume()
        loadChapters()
    }

    private fun loadChapters() {
        val pid = projectId ?: return
        listItems.clear()
        val volumes = workspaceRepository.getVolumes(pid)

        for (volume in volumes) {
            val chapters = workspaceRepository.getChapters(pid, volume.id)
            for (chapter in chapters) {
                listItems.add(
                    ChapterListItem(
                        volumeId = volume.id,
                        chapterId = chapter.id,
                        displayText = "${volume.title} - ${chapter.title}"
                    )
                )
            }
        }

        val displayTitles = listItems.map { it.displayText }
        val adapter = ArrayAdapter(this, R.layout.item_list, R.id.text1, displayTitles)
        chapterListView.adapter = adapter
    }

    private fun showNewChapterDialog() {
        val pid = projectId ?: return
        val volumes = workspaceRepository.getVolumes(pid)
        if (volumes.isEmpty()) {
            return
        }
        val defaultVolumeId = volumes.first().id

        val editText = EditText(this)
        editText.hint = "Chapter Title"

        AlertDialog.Builder(this)
            .setTitle("New Chapter")
            .setView(editText)
            .setPositiveButton("Create") { _, _ ->
                val title = editText.text.toString().trim()
                if (title.isNotEmpty()) {
                    workspaceRepository.createChapter(pid, defaultVolumeId, title)
                    loadChapters()
                }
            }
            .setNegativeButton("Cancel", null)
            .show()
    }
}
