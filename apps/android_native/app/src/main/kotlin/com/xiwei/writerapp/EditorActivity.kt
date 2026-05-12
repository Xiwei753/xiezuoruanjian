package com.xiwei.writerapp

import android.os.Bundle
import android.widget.Button
import android.widget.EditText
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity

class EditorActivity : AppCompatActivity() {
    private lateinit var editorEditText: EditText
    private lateinit var btnSave: Button
    private lateinit var workspaceRepository: WorkspaceRepository

    private var projectId: String? = null
    private var volumeId: String? = null
    private var chapterId: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_editor)

        editorEditText = findViewById(R.id.editorEditText)
        btnSave = findViewById(R.id.btnSave)
        workspaceRepository = WorkspaceRepository(this)

        projectId = intent.getStringExtra("PROJECT_ID")
        volumeId = intent.getStringExtra("VOLUME_ID")
        chapterId = intent.getStringExtra("CHAPTER_ID")

        if (projectId != null && volumeId != null && chapterId != null) {
            val content = workspaceRepository.getChapterContent(projectId!!, volumeId!!, chapterId!!)
            editorEditText.setText(content)
        } else {
            editorEditText.setText("Error: Missing chapter identifiers.")
            editorEditText.isEnabled = false
            btnSave.isEnabled = false
        }

        btnSave.setOnClickListener {
            saveContent()
            Toast.makeText(this, "Saved", Toast.LENGTH_SHORT).show()
        }
    }

    override fun onPause() {
        super.onPause()
        saveContent()
    }

    private fun saveContent() {
        val pid = projectId
        val vid = volumeId
        val cid = chapterId
        if (pid != null && vid != null && cid != null) {
            val content = editorEditText.text.toString()
            workspaceRepository.saveChapterContent(pid, vid, cid, content)
        }
    }
}
