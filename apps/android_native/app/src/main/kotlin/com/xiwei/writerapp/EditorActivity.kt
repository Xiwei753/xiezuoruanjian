package com.xiwei.writerapp

import android.os.Bundle
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity

class EditorActivity : AppCompatActivity() {
    private lateinit var editorTextView: TextView
    private lateinit var workspaceReader: SampleWorkspaceReader

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_editor)

        editorTextView = findViewById(R.id.editorTextView)
        workspaceReader = SampleWorkspaceReader(this)

        val projectId = intent.getStringExtra("PROJECT_ID")
        val volumeId = intent.getStringExtra("VOLUME_ID")
        val chapterId = intent.getStringExtra("CHAPTER_ID")

        if (projectId != null && volumeId != null && chapterId != null) {
            val content = workspaceReader.getChapterContent(projectId, volumeId, chapterId)
            editorTextView.text = content
        } else {
            editorTextView.text = "Error: Missing chapter identifiers."
        }
    }
}
