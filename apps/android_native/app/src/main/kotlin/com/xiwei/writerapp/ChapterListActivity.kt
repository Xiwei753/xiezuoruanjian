package com.xiwei.writerapp

import android.content.Intent
import android.os.Bundle
import android.widget.ArrayAdapter
import android.widget.ListView
import androidx.appcompat.app.AppCompatActivity

class ChapterListActivity : AppCompatActivity() {
    private lateinit var chapterListView: ListView
    private lateinit var workspaceReader: SampleWorkspaceReader

    // Simple class to store combined volume + chapter data for the list
    private data class ChapterListItem(val volumeId: String, val chapterId: String, val displayText: String)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_chapter_list)

        chapterListView = findViewById(R.id.chapterListView)
        workspaceReader = SampleWorkspaceReader(this)

        val projectId = intent.getStringExtra("PROJECT_ID") ?: return

        val listItems = mutableListOf<ChapterListItem>()
        val volumes = workspaceReader.getVolumes(projectId)

        for (volume in volumes) {
            val chapters = workspaceReader.getChapters(projectId, volume.id)
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

        chapterListView.setOnItemClickListener { _, _, position, _ ->
            val selectedItem = listItems[position]
            val intent = Intent(this, EditorActivity::class.java).apply {
                putExtra("PROJECT_ID", projectId)
                putExtra("VOLUME_ID", selectedItem.volumeId)
                putExtra("CHAPTER_ID", selectedItem.chapterId)
            }
            startActivity(intent)
        }
    }
}
