package com.xiwei.writerapp.ui

import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.Editable
import android.text.TextWatcher
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import com.google.android.material.appbar.MaterialToolbar
import android.content.Intent
import android.view.Menu
import android.view.MenuItem
import android.widget.TextView
import com.xiwei.writerapp.R
import com.xiwei.writerapp.data.SettingsRepository
import com.xiwei.writerapp.data.WorkspaceRepository

class EditorActivity : AppCompatActivity() {
    private lateinit var toolbar: MaterialToolbar
    private lateinit var editorEditText: WriterEditText

    private lateinit var workspaceRepository: WorkspaceRepository
    private lateinit var settingsRepository: SettingsRepository

    private var projectId: String? = null
    private var volumeId: String? = null
    private var chapterId: String? = null

    private var isDirty = false
    private val handler = Handler(Looper.getMainLooper())
    private var autoSaveEnabled = true
    private var autoSaveDelayMs = 1500L

    private lateinit var statusUnsaved: String

    private val autoSaveRunnable = Runnable { saveContent() }

    private lateinit var tvWordCount: TextView
    private lateinit var tvSessionAdded: TextView
    private lateinit var tvSpeed: TextView
    private lateinit var tvSaveStatus: TextView

    private var initialWordCount = 0
    private var sessionStartTime = System.currentTimeMillis()
    private var lastWordCount = 0

    private val statsUpdateRunnable = Runnable { updateStatsUI() }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_editor)

        toolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
        toolbar.setNavigationOnClickListener {
            onBackPressedDispatcher.onBackPressed()
        }

        editorEditText = findViewById(R.id.editorEditText)
        tvWordCount = findViewById(R.id.tvWordCount)
        tvSessionAdded = findViewById(R.id.tvSessionAdded)
        tvSpeed = findViewById(R.id.tvSpeed)
        tvSaveStatus = findViewById(R.id.tvSaveStatus)

        workspaceRepository = WorkspaceRepository(this)
        settingsRepository = SettingsRepository(this)

        // Load Settings
        val settings = settingsRepository.getLocalSettings()
        editorEditText.textSize = settings.editorFontSize
        editorEditText.setLineSpacing(0f, settings.editorLineSpacingMultiplier)
        editorEditText.setAutoIndent(settings.autoIndentEnabled, settings.autoIndentWidth)
        autoSaveEnabled = settings.autoSaveEnabled
        autoSaveDelayMs = settings.autoSaveDelayMs

        projectId = intent.getStringExtra("PROJECT_ID")
        volumeId = intent.getStringExtra("VOLUME_ID")
        chapterId = intent.getStringExtra("CHAPTER_ID")
        val chapterTitle = intent.getStringExtra("CHAPTER_TITLE")

        supportActionBar?.title = chapterTitle ?: getString(R.string.title_editor)

        if (projectId != null && volumeId != null && chapterId != null) {
            val content = ErrorUtil.safeRun(this, null as String?) {
                workspaceRepository.getChapterContent(projectId!!, volumeId!!, chapterId!!)
            }
            if (content != null) {
                editorEditText.setText(content)
                initialWordCount = calculateWordCount(content)
                lastWordCount = initialWordCount
                sessionStartTime = System.currentTimeMillis()
                updateStatsUI()
            } else {
                editorEditText.setText(getString(R.string.error_missing_chapter_identifiers))
                editorEditText.isEnabled = false
            }
        } else {
            editorEditText.setText(getString(R.string.error_missing_chapter_identifiers))
            editorEditText.isEnabled = false
        }

        statusUnsaved = getString(R.string.status_unsaved)
        toolbar.subtitle = "" // Clear toolbar subtitle

        editorEditText.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                if (projectId == null || volumeId == null || chapterId == null) return
                if (editorEditText.hasFocus()) {
                    isDirty = true
                    if (tvSaveStatus.text != statusUnsaved) {
                        tvSaveStatus.text = statusUnsaved
                    }
                    if (autoSaveEnabled) {
                        handler.removeCallbacks(autoSaveRunnable)
                        handler.postDelayed(autoSaveRunnable, autoSaveDelayMs)
                    }

                    // Debounce stats update to avoid lagging UI
                    handler.removeCallbacks(statsUpdateRunnable)
                    handler.postDelayed(statsUpdateRunnable, 500)
                }
            }
            override fun afterTextChanged(s: Editable?) {}
        })

        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                handler.removeCallbacks(autoSaveRunnable)
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

    override fun onCreateOptionsMenu(menu: Menu): Boolean {
        menuInflater.inflate(R.menu.menu_editor, menu)
        return true
    }

    override fun onOptionsItemSelected(item: MenuItem): Boolean {
        return when (item.itemId) {
            R.id.action_settings -> {
                val intent = Intent(this, SettingsActivity::class.java)
                startActivity(intent)
                true
            }
            else -> super.onOptionsItemSelected(item)
        }
    }

    override fun onResume() {
        super.onResume()
        val settings = settingsRepository.getLocalSettings()
        editorEditText.textSize = settings.editorFontSize
        editorEditText.setLineSpacing(0f, settings.editorLineSpacingMultiplier)
        editorEditText.setAutoIndent(settings.autoIndentEnabled, settings.autoIndentWidth)
        autoSaveEnabled = settings.autoSaveEnabled
        autoSaveDelayMs = settings.autoSaveDelayMs
    }

    override fun onPause() {
        super.onPause()
        handler.removeCallbacks(autoSaveRunnable)
        if (isDirty) {
            saveContent()
        }
    }

    override fun onStop() {
        super.onStop()
        handler.removeCallbacks(autoSaveRunnable)
        if (isDirty) {
            saveContent()
        }
    }

    private fun calculateWordCount(text: String): Int {
        return text.count { !it.isWhitespace() }
    }

    private fun updateStatsUI() {
        val content = editorEditText.text?.toString() ?: ""
        lastWordCount = calculateWordCount(content)

        val sessionAdded = lastWordCount - initialWordCount
        val elapsedMinutes = (System.currentTimeMillis() - sessionStartTime) / 60000.0
        val speed = if (elapsedMinutes > 0 && sessionAdded > 0) {
            (sessionAdded / elapsedMinutes).toInt()
        } else {
            0
        }

        tvWordCount.text = getString(R.string.stats_word_count, lastWordCount)
        tvSessionAdded.text = getString(R.string.stats_session_added, sessionAdded)
        tvSpeed.text = getString(R.string.stats_speed, speed)
    }

    private fun saveContent(): Boolean {
        val pid = projectId
        val vid = volumeId
        val cid = chapterId
        if (pid != null && vid != null && cid != null) {
            tvSaveStatus.text = getString(R.string.status_saving)
            val content = editorEditText.text.toString()
            val success = ErrorUtil.safeRun(this, false) {
                workspaceRepository.saveChapterContent(pid, vid, cid, content)
            }
            if (success) {
                isDirty = false
                tvSaveStatus.text = getString(R.string.status_saved)
                return true
            } else {
                tvSaveStatus.text = getString(R.string.status_save_failed)
                return false
            }
        }
        return false
    }
}
