package com.xiwei.writerapp.ui

import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.Editable
import android.text.TextWatcher
import android.widget.EditText
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import com.google.android.material.appbar.MaterialToolbar
import com.xiwei.writerapp.R
import com.xiwei.writerapp.data.SettingsRepository
import com.xiwei.writerapp.data.WorkspaceRepository

class EditorActivity : AppCompatActivity() {
    private lateinit var toolbar: MaterialToolbar
    private lateinit var editorEditText: EditText

    private lateinit var workspaceRepository: WorkspaceRepository
    private lateinit var settingsRepository: SettingsRepository

    private var projectId: String? = null
    private var volumeId: String? = null
    private var chapterId: String? = null

    private var isDirty = false
    private val handler = Handler(Looper.getMainLooper())
    private var autoSaveEnabled = true
    private var autoSaveDelayMs = 1500L

    private val autoSaveRunnable = Runnable { saveContent() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_editor)

        toolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
        toolbar.setNavigationOnClickListener {
            onBackPressedDispatcher.onBackPressed()
        }

        editorEditText = findViewById(R.id.editorEditText)
        workspaceRepository = WorkspaceRepository(this)
        settingsRepository = SettingsRepository(this)

        // Load Settings
        val settings = settingsRepository.getLocalSettings()
        editorEditText.textSize = settings.editorFontSize
        editorEditText.setLineSpacing(0f, settings.editorLineSpacingMultiplier)
        autoSaveEnabled = settings.autoSaveEnabled
        autoSaveDelayMs = settings.autoSaveDelayMs

        projectId = intent.getStringExtra("PROJECT_ID")
        volumeId = intent.getStringExtra("VOLUME_ID")
        chapterId = intent.getStringExtra("CHAPTER_ID")
        val chapterTitle = intent.getStringExtra("CHAPTER_TITLE")

        supportActionBar?.title = chapterTitle ?: "Editor"

        if (projectId != null && volumeId != null && chapterId != null) {
            val content = workspaceRepository.getChapterContent(projectId!!, volumeId!!, chapterId!!)
            editorEditText.setText(content)
        } else {
            editorEditText.setText("Error: Missing chapter identifiers.")
            editorEditText.isEnabled = false
        }

        editorEditText.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                if (projectId == null || volumeId == null || chapterId == null) return
                if (editorEditText.hasFocus()) {
                    isDirty = true
                    toolbar.subtitle = getString(R.string.status_unsaved)
                    if (autoSaveEnabled) {
                        handler.removeCallbacks(autoSaveRunnable)
                        handler.postDelayed(autoSaveRunnable, autoSaveDelayMs)
                    }
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
                            .setTitle(R.string.dialog_save_failed_title)
                            .setMessage(R.string.dialog_save_failed_message)
                            .setPositiveButton(R.string.action_exit) { _, _ -> finish() }
                            .setNegativeButton(R.string.action_retry) { _, _ -> saveContent() }
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
            toolbar.subtitle = getString(R.string.status_saving)
            val content = editorEditText.text.toString()
            val success = workspaceRepository.saveChapterContent(pid, vid, cid, content)
            if (success) {
                isDirty = false
                toolbar.subtitle = getString(R.string.status_saved)
                return true
            } else {
                toolbar.subtitle = getString(R.string.status_save_failed)
                return false
            }
        }
        return false
    }
}
