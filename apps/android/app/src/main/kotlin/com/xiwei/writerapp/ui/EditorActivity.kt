package com.xiwei.writerapp.ui

import android.os.Bundle
import android.text.Editable
import android.text.TextWatcher
import androidx.activity.OnBackPressedCallback
import androidx.activity.viewModels
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
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
import android.widget.Toast
import com.xiwei.writerapp.R
import com.xiwei.writerapp.data.SyncChangeBus

/**
 * EditorActivity — 章节编辑器页面
 *
 * 提供纯文本编辑环境，集成平滑光标、打字动画、自动缩进等写作增强功能。
 *
 * ## 架构定位
 * - EditorActivity → EditorViewModel → WorkspaceRepository → NativeCoreBridge → Rust Core
 * - EditorActivity → WriterEditText → EditorAnimationRuntime
 *
 * ## 职责边界
 * - **做**：文本编辑、自动保存、工具栏交互、星图链接
 * - **不做**：文件 I/O（由 Rust Core 负责）、排版格式化（由 WriterEditText 负责）
 *
 * ## 使用场景
 * - 用户点击章节后进入编辑器
 * - 进行纯文本写作和编辑
 */
class EditorActivity : AppCompatActivity() {
import com.xiwei.writerapp.data.WorkspaceRepository
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import com.xiwei.writerapp.data.SettingsChangeBus

class EditorActivity : AppCompatActivity() {
    private lateinit var toolbar: MaterialToolbar
    private lateinit var editorEditText: WriterEditText

    private val viewModel: EditorViewModel by viewModels()

    private var projectId: String? = null
    private var volumeId: String? = null
    private var chapterId: String? = null

    private lateinit var tvWordCount: TextView
    private lateinit var tvSessionAdded: TextView
    private lateinit var tvSpeed: TextView
    private lateinit var tvSaveStatus: TextView

    private var textWatcher: TextWatcher? = null
    private lateinit var workspaceRepository: WorkspaceRepository

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

            appBarLayout.setPadding(0, systemBarsInsets.top, 0, 0)

            val bottomInset = maxOf(imeInsets.bottom, systemBarsInsets.bottom)
            val params = editorStatusBar.layoutParams as android.view.ViewGroup.MarginLayoutParams
            params.bottomMargin = bottomInset + (16 * resources.displayMetrics.density).toInt()
            editorStatusBar.layoutParams = params

            editorStatusBar.post {
                val statsBarHeight = editorStatusBar.height
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

        searchLayout = findViewById(R.id.searchLayout)
        etSearch = findViewById(R.id.etSearch)
        etReplace = findViewById(R.id.etReplace)
        btnSearchNext = findViewById(R.id.btnSearchNext)
        btnSearchClose = findViewById(R.id.btnSearchClose)
        btnReplace = findViewById(R.id.btnReplace)
        btnReplaceAll = findViewById(R.id.btnReplaceAll)

        setupSearchAndReplace()
        observeViewModel()
        setupTextWatcher()
        setupBackPressed()
        workspaceRepository = WorkspaceRepository(this)

        projectId = intent.getStringExtra("PROJECT_ID")
        volumeId = intent.getStringExtra("VOLUME_ID")
        chapterId = intent.getStringExtra("CHAPTER_ID")
        val chapterTitle = intent.getStringExtra("CHAPTER_TITLE")

        supportActionBar?.title = chapterTitle ?: getString(R.string.title_editor)

        if (projectId != null && volumeId != null && chapterId != null) {
            viewModel.initChapter(projectId!!, volumeId!!, chapterId!!, chapterTitle ?: "")
        } else {
            viewModel.initErrorState(getString(R.string.error_missing_chapter_identifiers))
        }
    }

    private fun observeViewModel() {
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                viewModel.uiState.collectLatest { state ->
                    if (state.loading) {
                        tvSaveStatus.text = ""
                        return@collectLatest
                    }

                    if (!editorEditText.hasFocus()) {
                        editorEditText.runWithoutTextAnimations {
                            if (editorEditText.text?.toString() != state.content) {
                                editorEditText.setText(state.content)
                            }
                        }
                    }

                    editorEditText.isEnabled = state.editorEnabled

                    when (state.saveStatus) {
                        SaveStatus.Idle -> tvSaveStatus.text = ""
                        SaveStatus.Unsaved -> tvSaveStatus.text = getString(R.string.status_unsaved)
                        SaveStatus.Saving -> tvSaveStatus.text = getString(R.string.status_saving)
                        SaveStatus.Saved -> tvSaveStatus.text = getString(R.string.status_saved)
                        SaveStatus.SaveFailed -> tvSaveStatus.text = getString(R.string.status_save_failed)
                    }

                    tvWordCount.text = getString(R.string.stats_word_count, state.wordCount)
                    tvSessionAdded.text = getString(R.string.stats_session_added, state.sessionAdded)
                    tvSpeed.text = getString(R.string.stats_speed, state.speed)

                    applySettingsToEditor(state.settings)
                }
            }
        }

        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                viewModel.events.collectLatest { event ->
                    when (event) {
                        is EditorEvent.ToastMessage -> {
                            Toast.makeText(this@EditorActivity, event.message, Toast.LENGTH_LONG).show()
                        }
                        is EditorEvent.ShowSaveFailedDialog -> {
                            showSaveFailedDialog(event.message)
                        }
                    }
                }
            }
        }
    }

    private var lastFontSize: Float? = null
    private var lastLineSpacing: Float? = null

    private fun applySettingsToEditor(settings: EditorSettingsState) {
        if (lastFontSize != settings.fontSize) {
            lastFontSize = settings.fontSize
            editorEditText.textSize = settings.fontSize
        }
        if (lastLineSpacing != settings.lineSpacingMultiplier) {
            lastLineSpacing = settings.lineSpacingMultiplier
            editorEditText.setLineSpacing(0f, settings.lineSpacingMultiplier)
        }
        editorEditText.setAutoIndent(settings.autoIndentEnabled, settings.autoIndentWidth)
        editorEditText.setTypingAnimationEnabled(settings.typingAnimationEnabled, settings.typingAnimationDurationMs)
        editorEditText.setSmoothCursorEnabled(settings.smoothCursorEnabled, settings.smoothCursorDurationMs)
    }

    private fun setupTextWatcher() {
        textWatcher = object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {
                if (projectId == null || volumeId == null || chapterId == null) return
                if (editorEditText.hasFocus()) {
                    viewModel.onContentChanged(editorEditText.text?.toString() ?: "")
                }
            }
            override fun afterTextChanged(s: Editable?) {}
        }
        textWatcher?.let { editorEditText.addTextChangedListener(it) }
    }

    private fun setupBackPressed() {
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                val currentState = viewModel.uiState.value
                if (currentState.saveStatus == SaveStatus.Unsaved || currentState.saveStatus == SaveStatus.SaveFailed) {
                    lifecycleScope.launch {
                        val success = viewModel.requestSave().await()
                        if (success) {
                            finish()
                        }
                    }
                } else if (currentState.saveStatus == SaveStatus.Saving) {
                    return
                } else {
                    finish()
                }
            }
        })
    }

    private fun showSaveFailedDialog(message: String) {
        AlertDialog.Builder(this)
            .setTitle(R.string.dialog_save_failed_title)
            .setMessage(message)
            .setPositiveButton(R.string.action_exit) { _, _ -> finish() }
            .setNegativeButton(R.string.action_retry) { _, _ ->
                lifecycleScope.launch {
                    val success = viewModel.requestSave().await()
                    if (success) {
                        finish()
                    }
                }
            }
            .show()
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
            R.id.action_registry -> {
                val intent = Intent(this, ActionRegistryActivity::class.java)
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

        val currentNote = viewModel.uiState.value.chapterNote ?: ""

        val editText = EditText(this)
        editText.hint = getString(R.string.hint_chapter_note)
        editText.setPadding(48, 48, 48, 48)
        editText.inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
        editText.minLines = 3
        editText.maxLines = 10
        editText.gravity = android.view.Gravity.TOP or android.view.Gravity.START
        editText.setText(currentNote)

        AlertDialog.Builder(this)
            .setTitle(R.string.dialog_chapter_note_title)
            .setView(editText)
            .setPositiveButton(R.string.action_save) { _, _ ->
                val newNote = editText.text.toString().trim()
                viewModel.updateChapterNote(newNote)
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    override fun onResume() {
        super.onResume()
        if (!::editorEditText.isInitialized) return
        if (SyncChangeBus.consumeChanged()) {
            val pid = projectId
            val vid = volumeId
            val cid = chapterId
            if (pid != null && vid != null && cid != null) {
                try {
                    workspaceRepository.getChapterContentWithMeta(pid, vid, cid)
                } catch (_: Exception) {
                    Toast.makeText(this, "当前章节已在其他设备删除，已返回列表。", Toast.LENGTH_LONG).show()
                    finish()
                    return
                }
            }
        }
        if (SettingsChangeBus.consumeEditorChanged()) {
            viewModel.onSettingsChanged()
        }
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
                    performSearch()
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
}
