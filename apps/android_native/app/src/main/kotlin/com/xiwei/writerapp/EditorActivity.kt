package com.xiwei.writerapp

import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.Editable
import android.text.TextWatcher
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import android.widget.Toast
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity

class EditorActivity : AppCompatActivity() {
    private lateinit var editorEditText: EditText
    private lateinit var btnSave: Button
    private lateinit var tvSaveStatus: TextView
    private lateinit var workspaceRepository: WorkspaceRepository

    private var projectId: String? = null
    private var volumeId: String? = null
    private var chapterId: String? = null

    private var isDirty = false
    private val handler = Handler(Looper.getMainLooper())
    private val autoSaveRunnable = Runnable { saveContent() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_editor)

        editorEditText = findViewById(R.id.editorEditText)
        btnSave = findViewById(R.id.btnSave)
        tvSaveStatus = findViewById(R.id.tvSaveStatus)
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
        }

        editorEditText.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                if (projectId == null || volumeId == null || chapterId == null) return
                if (editorEditText.hasFocus()) {
                    isDirty = true
                    tvSaveStatus.text = "Unsaved"
                    handler.removeCallbacks(autoSaveRunnable)
                    handler.postDelayed(autoSaveRunnable, 1500)
                }
            }
            override fun afterTextChanged(s: Editable?) {}
        })

        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                if (isDirty) {
                    val success = saveContent()
                    if (success) {
                        finish()
                    } else {
                        AlertDialog.Builder(this@EditorActivity)
                            .setTitle("Save Failed")
                            .setMessage("Failed to save the chapter. Do you want to exit without saving?")
                            .setPositiveButton("Exit") { _, _ -> finish() }
                            .setNegativeButton("Retry") { _, _ -> saveContent() }
                            .show()
                    }
                } else {
                    finish()
                }
            }
        })
    }

    override fun onPause() {
        super.onPause()
        if (isDirty) {
            saveContent()
        }
    }

    override fun onStop() {
        super.onStop()
        if (isDirty) {
            saveContent()
        }
    }

    private fun saveContent(): Boolean {
        val pid = projectId
        val vid = volumeId
        val cid = chapterId
        if (pid != null && vid != null && cid != null) {
            tvSaveStatus.text = "Saving..."
            val content = editorEditText.text.toString()
            val success = workspaceRepository.saveChapterContent(pid, vid, cid, content)
            if (success) {
                isDirty = false
                tvSaveStatus.text = "Saved"
                return true
            } else {
                tvSaveStatus.text = "Save Failed"
                return false
            }
        }
        return false
    }
}
