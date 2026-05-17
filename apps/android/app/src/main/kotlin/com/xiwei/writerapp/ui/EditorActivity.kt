package com.xiwei.writerapp.ui

import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.Editable
import android.text.TextWatcher
import androidx.activity.OnBackPressedCallback
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import com.google.android.material.appbar.MaterialToolbar
import android.content.Intent
import android.view.Menu
import android.view.MenuItem
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.TextView
import android.text.Spannable
import android.text.style.BackgroundColorSpan
import android.util.Log
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
    private var currentChapterNote: String? = null

    private val statsUpdateRunnable = Runnable { updateStatsUI() }

    // Search and Replace
    private lateinit var searchLayout: LinearLayout
    private lateinit var etSearch: EditText
    private lateinit var etReplace: EditText
    private lateinit var btnSearchNext: ImageButton
    private lateinit var btnSearchClose: ImageButton
    private lateinit var btnReplace: Button
    private lateinit var btnReplaceAll: Button

    private var searchResults = mutableListOf<Pair<Int, Int>>()
    private var currentSearchIndex = -1
    private val highlightSpans = mutableListOf<BackgroundColorSpan>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        androidx.core.view.WindowCompat.setDecorFitsSystemWindows(window, false)
        setContentView(R.layout.activity_editor)

        window.decorView.post {
            UiFontUtil.applySansSerifFallback(window.decorView.rootView)
        }

        val mainLayout = findViewById<View>(R.id.editorCoordinatorLayout)
        val appBarLayout = findViewById<View>(R.id.appBarLayout)
        val editorStatusBar = findViewById<View>(R.id.editorStatusBar)
        editorEditText = findViewById(R.id.editorEditText)

        ViewCompat.setOnApplyWindowInsetsListener(mainLayout) { view, insets ->
            val imeInsets = insets.getInsets(WindowInsetsCompat.Type.ime())
            val systemBarsInsets = insets.getInsets(WindowInsetsCompat.Type.systemBars())

            // Top padding for AppBar to sit below status bar
            appBarLayout.setPadding(0, systemBarsInsets.top, 0, 0)

            // Stats bar needs to sit above nav bar and IME
            val bottomInset = maxOf(imeInsets.bottom, systemBarsInsets.bottom)
            val params = editorStatusBar.layoutParams as android.view.ViewGroup.MarginLayoutParams
            params.bottomMargin = bottomInset + (16 * resources.displayMetrics.density).toInt()
            editorStatusBar.layoutParams = params

            // Editor padding needs to account for stats bar + inset
            editorStatusBar.post {
                val statsBarHeight = editorStatusBar.height
                // 32dp extra padding so we can scroll past last line
                val extraPadding = (32 * resources.displayMetrics.density).toInt()
                editorEditText.setPadding(
                    editorEditText.paddingLeft,
                    editorEditText.paddingTop,
                    editorEditText.paddingRight,
                    statsBarHeight + bottomInset + extraPadding
                )
            }

            WindowInsetsCompat.CONSUMED
        }

        toolbar = findViewById(R.id.toolbar)
        setSupportActionBar(toolbar)
        toolbar.setNavigationOnClickListener {
            onBackPressedDispatcher.onBackPressed()
        }


        editorEditText.typeface = android.graphics.Typeface.create("sans-serif", android.graphics.Typeface.NORMAL)
        tvWordCount = findViewById(R.id.tvWordCount)
        tvSessionAdded = findViewById(R.id.tvSessionAdded)
        tvSpeed = findViewById(R.id.tvSpeed)
        tvSaveStatus = findViewById(R.id.tvSaveStatus)

        workspaceRepository = WorkspaceRepository(this)
        settingsRepository = SettingsRepository(this)

        searchLayout = findViewById(R.id.searchLayout)
        etSearch = findViewById(R.id.etSearch)
        etReplace = findViewById(R.id.etReplace)
        btnSearchNext = findViewById(R.id.btnSearchNext)
        btnSearchClose = findViewById(R.id.btnSearchClose)
        btnReplace = findViewById(R.id.btnReplace)
        btnReplaceAll = findViewById(R.id.btnReplaceAll)

        setupSearchAndReplace()

        // Load Settings
        val settings = settingsRepository.getLocalSettings()
        editorEditText.textSize = settings.editorFontSize
        editorEditText.setLineSpacing(0f, settings.editorLineSpacingMultiplier)
        editorEditText.setAutoIndent(settings.autoIndentEnabled, settings.autoIndentWidth)
        editorEditText.setTypingAnimationEnabled(settings.editorTypingAnimationEnabled, settings.editorTypingAnimationDurationMs.toLong())
        editorEditText.setSmoothCursorEnabled(settings.editorSmoothCursorEnabled, settings.editorSmoothCursorDurationMs.toLong())
        autoSaveEnabled = settings.autoSaveEnabled
        autoSaveDelayMs = settings.autoSaveDelayMs

        projectId = intent.getStringExtra("PROJECT_ID")
        volumeId = intent.getStringExtra("VOLUME_ID")
        chapterId = intent.getStringExtra("CHAPTER_ID")
        val chapterTitle = intent.getStringExtra("CHAPTER_TITLE")

        supportActionBar?.title = chapterTitle ?: getString(R.string.title_editor)

        if (projectId != null && volumeId != null && chapterId != null) {
            val result = ErrorUtil.safeRun(this, null) {
                workspaceRepository.getChapterContentWithMeta(projectId!!, volumeId!!, chapterId!!)
            }
            if (result != null) {
                val content = result.first
                val meta = result.second
                currentChapterNote = meta.note

                editorEditText.runWithoutTextAnimations {
                    editorEditText.setText(content)
                }

                initialWordCount = calculateWordCount(content)
                lastWordCount = initialWordCount
                sessionStartTime = System.currentTimeMillis()
                updateStatsUI()

                // Record recent edit
                ErrorUtil.safeRun(this) {
                    workspaceRepository.recordRecentEdit(projectId!!, volumeId!!, chapterId!!)
                }
            } else {
                editorEditText.runWithoutTextAnimations {
                    editorEditText.setText(getString(R.string.error_missing_chapter_identifiers))
                }
                editorEditText.isEnabled = false
            }
        } else {
            editorEditText.runWithoutTextAnimations {
                editorEditText.setText(getString(R.string.error_missing_chapter_identifiers))
            }
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
            R.id.action_note -> {
                showChapterNoteDialog()
                true
            }
            R.id.action_search -> {
                toggleSearchBar()
                true
            }
            R.id.action_settings -> {
                val intent = Intent(this, SettingsActivity::class.java)
                startActivity(intent)
                true
            }
            else -> super.onOptionsItemSelected(item)
        }
    }

    private fun showChapterNoteDialog() {
        val pid = projectId
        val vid = volumeId
        val cid = chapterId
        if (pid == null || vid == null || cid == null) return

        val editText = EditText(this)
        editText.hint = getString(R.string.hint_chapter_note)
        editText.setPadding(48, 48, 48, 48)
        editText.inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
        editText.minLines = 3
        editText.maxLines = 10
        editText.gravity = android.view.Gravity.TOP or android.view.Gravity.START
        editText.setText(currentChapterNote)

        AlertDialog.Builder(this)
            .setTitle(R.string.dialog_chapter_note_title)
            .setView(editText)
            .setPositiveButton(R.string.action_save) { _, _ ->
                val newNote = editText.text.toString().trim()
                ErrorUtil.safeRun(this) {
                    workspaceRepository.updateChapterNote(pid, vid, cid, newNote)
                    currentChapterNote = newNote
                }
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    override fun onResume() {
        super.onResume()
        if (!::editorEditText.isInitialized) return

        ErrorUtil.safeRun(this) {
            val settings = settingsRepository.getLocalSettings()
            editorEditText.textSize = settings.editorFontSize
            editorEditText.setLineSpacing(0f, settings.editorLineSpacingMultiplier)
            editorEditText.setAutoIndent(settings.autoIndentEnabled, settings.autoIndentWidth)
            editorEditText.setTypingAnimationEnabled(settings.editorTypingAnimationEnabled, settings.editorTypingAnimationDurationMs.toLong())
            editorEditText.setSmoothCursorEnabled(settings.editorSmoothCursorEnabled, settings.editorSmoothCursorDurationMs.toLong())
            autoSaveEnabled = settings.autoSaveEnabled
            autoSaveDelayMs = settings.autoSaveDelayMs
        }
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

    private fun setupSearchAndReplace() {
        btnSearchClose.setOnClickListener {
            searchLayout.visibility = View.GONE
            clearHighlights()
        }

        etSearch.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                performSearch()
            }
            override fun afterTextChanged(s: Editable?) {}
        })

        btnSearchNext.setOnClickListener {
            if (searchResults.isNotEmpty()) {
                currentSearchIndex = (currentSearchIndex + 1) % searchResults.size
                focusSearchResult()
            }
        }

        btnReplace.setOnClickListener {
            if (searchResults.isNotEmpty() && currentSearchIndex in searchResults.indices) {
                val replaceStr = etReplace.text.toString()
                val currentMatch = searchResults[currentSearchIndex]
                val editable = editorEditText.text
                if (editable != null) {
                    editorEditText.runWithoutTextAnimations {
                        editable.replace(currentMatch.first, currentMatch.second, replaceStr)
                    }
                    // The text watcher on editorEditText will trigger performSearch(),
                    // which resets indices. So we just let that handle the update.
                }
            }
        }

        btnReplaceAll.setOnClickListener {
            val searchStr = etSearch.text.toString()
            val replaceStr = etReplace.text.toString()
            if (searchStr.isNotEmpty()) {
                val editable = editorEditText.text
                if (editable != null) {
                    val content = editable.toString()
                    val newContent = content.replace(searchStr, replaceStr)
                    editorEditText.runWithoutTextAnimations {
                        editorEditText.setText(newContent)
                    }
                    performSearch() // to clear/update results
                }
            }
        }
    }

    private fun toggleSearchBar() {
        if (searchLayout.visibility == View.VISIBLE) {
            searchLayout.visibility = View.GONE
            clearHighlights()
        } else {
            searchLayout.visibility = View.VISIBLE
            etSearch.requestFocus()
            performSearch()
        }
    }

    private fun performSearch() {
        clearHighlights()
        searchResults.clear()
        currentSearchIndex = -1

        if (searchLayout.visibility == View.GONE) return

        val searchStr = etSearch.text.toString()
        if (searchStr.isEmpty()) return

        val content = editorEditText.text?.toString() ?: return
        var startIndex = content.indexOf(searchStr)
        val highlightColor = getColor(com.google.android.material.R.color.material_dynamic_primary70)

        while (startIndex >= 0) {
            val endIndex = startIndex + searchStr.length
            searchResults.add(Pair(startIndex, endIndex))

            val span = BackgroundColorSpan(highlightColor)
            editorEditText.text?.setSpan(span, startIndex, endIndex, Spannable.SPAN_EXCLUSIVE_EXCLUSIVE)
            highlightSpans.add(span)

            startIndex = content.indexOf(searchStr, endIndex)
        }

        if (searchResults.isNotEmpty()) {
            currentSearchIndex = 0
            focusSearchResult()
        }
    }

    private fun focusSearchResult() {
        if (currentSearchIndex in searchResults.indices) {
            val match = searchResults[currentSearchIndex]
            editorEditText.setSelection(match.second)

            // Highlight current match differently
            clearHighlights()
            val highlightColor = getColor(com.google.android.material.R.color.material_dynamic_primary70)
            val activeColor = getColor(com.google.android.material.R.color.material_dynamic_primary50)

            for (i in searchResults.indices) {
                val res = searchResults[i]
                val color = if (i == currentSearchIndex) activeColor else highlightColor
                val span = BackgroundColorSpan(color)
                editorEditText.text?.setSpan(span, res.first, res.second, Spannable.SPAN_EXCLUSIVE_EXCLUSIVE)
                highlightSpans.add(span)
            }
        }
    }

    private fun clearHighlights() {
        val editable = editorEditText.text ?: return
        for (span in highlightSpans) {
            editable.removeSpan(span)
        }
        highlightSpans.clear()
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
