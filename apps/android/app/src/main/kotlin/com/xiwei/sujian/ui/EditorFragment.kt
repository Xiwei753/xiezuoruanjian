package com.xiwei.sujian.ui

import android.os.Bundle
import android.text.Editable
import android.text.Spannable
import android.text.style.BackgroundColorSpan
import com.xiwei.sujian.diagnostics.DiagnosticsLogger
import android.view.LayoutInflater
import android.view.Menu
import android.view.MenuInflater
import android.view.MenuItem
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AlertDialog
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.fragment.app.Fragment
import androidx.fragment.app.viewModels
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.xiwei.sujian.R
import com.xiwei.sujian.data.CoreSettingsEvents
import com.xiwei.sujian.data.SyncChangeBus
import com.xiwei.sujian.data.WorkspaceRepository
import com.xiwei.sujian.editor.selfrender.SujianEditorView
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch

/**
 * EditorFragment — 可嵌入的章节编辑器 Fragment
 *
 * 从 EditorActivity 提取编辑器核心逻辑，支持两种使用模式：
 * - **TwoPane 模式**：嵌入 MainActivity 右侧面板（detailContentContainer）
 * - **SinglePane 模式**：由 EditorActivity 包裹，提供独立 toolbar
 *
 * ## 架构定位
 * - EditorFragment → EditorViewModel → WorkspaceRepository → 领域 Bridge → Rust Core
 * - EditorFragment → WriterEditText → EditorAnimationRuntime
 *
 * ## 职责边界
 * - **做**：文本编辑、自动保存、搜索/替换、工具栏交互
 * - **不做**：文件 I/O（由 Rust Core 负责）、排版格式化（由 WriterEditText 负责）
 *
 * ## 宿主通信
 * - 通过 `EditorFragmentCallback` 回调宿主（如 back 请求）
 * - 通过 `initChapter()` 由宿主初始化章节
 * - 通过 `requestSave()` 由宿主触发保存
 */
class EditorFragment : Fragment() {

    companion object {
        const val ARG_PROJECT_ID = "PROJECT_ID"
        const val ARG_VOLUME_ID = "VOLUME_ID"
        const val ARG_CHAPTER_ID = "CHAPTER_ID"
        const val ARG_CHAPTER_TITLE = "CHAPTER_TITLE"

        /**
         * 工厂方法：创建带参数的 EditorFragment 实例
         */
        fun newInstance(
            projectId: String,
            volumeId: String,
            chapterId: String,
            chapterTitle: String
        ): EditorFragment {
            return EditorFragment().apply {
                arguments = Bundle().apply {
                    putString(ARG_PROJECT_ID, projectId)
                    putString(ARG_VOLUME_ID, volumeId)
                    putString(ARG_CHAPTER_ID, chapterId)
                    putString(ARG_CHAPTER_TITLE, chapterTitle)
                }
            }
        }
    }

    /**
     * 宿主回调接口
     */
    interface EditorFragmentCallback {
        /** 用户请求返回（如 back 键、toolbar 导航按钮） */
        fun onBackRequested()
    }

    // ── Views ──
    private lateinit var editorEditText: WriterEditText
    private lateinit var sujianEditorView: SujianEditorView
    private lateinit var tvWordCount: TextView
    private lateinit var tvSessionAdded: TextView
    private lateinit var tvSpeed: TextView
    private lateinit var tvSaveStatus: TextView

    // ── Self-render editor switch ──
    private var useSelfRenderEditor: Boolean = false

    // Search and Replace
    private lateinit var searchLayout: LinearLayout
    private lateinit var etSearch: EditText
    private lateinit var etReplace: EditText
    private lateinit var btnSearchNext: ImageButton
    private lateinit var btnSearchClose: ImageButton
    private lateinit var btnReplace: Button
    private lateinit var btnReplaceAll: Button

    // ── State ──
    private val viewModel: EditorViewModel by viewModels()
    private var projectId: String? = null
    private var volumeId: String? = null
    private var chapterId: String? = null
    private var textWatcher: android.text.TextWatcher? = null
    private lateinit var workspaceRepository: WorkspaceRepository

    private var searchResults = mutableListOf<Pair<Int, Int>>()
    private var currentSearchIndex = -1
    private val highlightSpans = mutableListOf<BackgroundColorSpan>()

    private var callback: EditorFragmentCallback? = null

    // ── Settings cache ──
    private var lastFontSize: Float? = null
    private var lastLineSpacing: Float? = null
    private var lastTypingAnimEnabled: Boolean? = null
    private var lastTypingAnimDuration: Long? = null
    private var lastSmoothCursorEnabled: Boolean? = null
    private var lastSmoothCursorDuration: Long? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setHasOptionsMenu(true)

        // 从 arguments 读取初始参数
        projectId = arguments?.getString(ARG_PROJECT_ID)
        volumeId = arguments?.getString(ARG_VOLUME_ID)
        chapterId = arguments?.getString(ARG_CHAPTER_ID)
    }

    override fun onCreateView(
        inflater: LayoutInflater,
        container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View? {
        return inflater.inflate(R.layout.fragment_editor, container, false)
    }

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        super.onViewCreated(view, savedInstanceState)

        workspaceRepository = WorkspaceRepository(requireContext())

        // ── 读取自研写作区开关 ──
        useSelfRenderEditor = try {
            val settings = com.xiwei.sujian.data.SettingsRepository(requireContext()).getLocalSettings()
            settings.useSelfRenderEditorOnAndroid
        } catch (_: Exception) {
            false
        }

        // ── Bind views ──
        editorEditText = view.findViewById(R.id.editorEditText)
        sujianEditorView = view.findViewById(R.id.sujianEditorView)
        tvWordCount = view.findViewById(R.id.tvWordCount)
        tvSessionAdded = view.findViewById(R.id.tvSessionAdded)
        tvSpeed = view.findViewById(R.id.tvSpeed)
        tvSaveStatus = view.findViewById(R.id.tvSaveStatus)

        searchLayout = view.findViewById(R.id.searchLayout)
        etSearch = view.findViewById(R.id.etSearch)
        etReplace = view.findViewById(R.id.etReplace)
        btnSearchNext = view.findViewById(R.id.btnSearchNext)
        btnSearchClose = view.findViewById(R.id.btnSearchClose)
        btnReplace = view.findViewById(R.id.btnReplace)
        btnReplaceAll = view.findViewById(R.id.btnReplaceAll)

        // ── 根据开关切换编辑器 ──
        if (useSelfRenderEditor) {
            editorEditText.visibility = View.GONE
            sujianEditorView.visibility = View.VISIBLE
            setupSelfRenderEditor()
        } else {
            editorEditText.visibility = View.VISIBLE
            sujianEditorView.visibility = View.GONE
            setupLegacyEditor()
        }

        // ── Apply font fallback ──
        view.post {
            UiFontUtil.applySansSerifFallback(view)
        }

        // ── Window insets ──
        val editorStatusBar = view.findViewById<View>(R.id.editorStatusBar)
        ViewCompat.setOnApplyWindowInsetsListener(view) { v, insets ->
            val imeInsets = insets.getInsets(WindowInsetsCompat.Type.ime())
            val systemBarsInsets = insets.getInsets(WindowInsetsCompat.Type.systemBars())

            val bottomInset = maxOf(imeInsets.bottom, systemBarsInsets.bottom)
            val params = editorStatusBar.layoutParams as android.view.ViewGroup.MarginLayoutParams
            params.bottomMargin = bottomInset + (16 * resources.displayMetrics.density).toInt()
            editorStatusBar.layoutParams = params

            if (!useSelfRenderEditor) {
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
            }

            WindowInsetsCompat.CONSUMED
        }

        setupSearchAndReplace()
        observeViewModel()
        if (!useSelfRenderEditor) {
            setupTextWatcher()
        }

        // ── Initialize chapter from arguments if available ──
        val chapterTitle = arguments?.getString(ARG_CHAPTER_TITLE) ?: ""
        if (projectId != null && volumeId != null && chapterId != null) {
            viewModel.initChapter(projectId!!, volumeId!!, chapterId!!, chapterTitle)
        } else {
            viewModel.initErrorState(getString(R.string.error_missing_chapter_identifiers))
        }
    }

    /**
     * 设置旧版 WriterEditText 编辑器
     */
    private fun setupLegacyEditor() {
        editorEditText.typeface = android.graphics.Typeface.create("sans-serif", android.graphics.Typeface.NORMAL)

        try {
            val animBridge = com.xiwei.sujian.data.BridgeProvider.getEditorAnimationBridge(requireContext())
            editorEditText.setAnimationEventProvider(AnimationEventProvider { oldText, newText, oldCursorIndex, newCursorIndex, cause, maxAnimatedChars, animationDurationMs ->
                try {
                    when (val result = animBridge.editorAnimationEvents(oldText, newText, oldCursorIndex, newCursorIndex, cause, maxAnimatedChars, animationDurationMs)) {
                        is com.xiwei.sujian.data.BridgeResult.Success -> {
                            editorEditText.typingAnimationController?.providerFailedLastTime = false
                            result.data
                        }
                        else -> {
                            val typingEnabled = editorEditText.typingAnimationController?.typingAnimationEnabled ?: false
                            DiagnosticsLogger.w("WriterSettings", "AnimationEventProvider returned failure: typingEnabled=$typingEnabled, providerInjected=true")
                            editorEditText.typingAnimationController?.providerFailedLastTime = true
                            emptyList()
                        }
                    }
                } catch (e: Exception) {
                    val typingEnabled = editorEditText.typingAnimationController?.typingAnimationEnabled ?: false
                    DiagnosticsLogger.w("WriterSettings", "AnimationEventProvider threw exception: typingEnabled=$typingEnabled, providerInjected=true, exception=${e.message}", e)
                    editorEditText.typingAnimationController?.providerFailedLastTime = true
                    emptyList()
                }
            })
            DiagnosticsLogger.d("WriterSettings", "AnimationEventProvider injected from EditorAnimationBridge")
        } catch (e: Exception) {
            val typingEnabled = editorEditText.typingAnimationController?.typingAnimationEnabled ?: false
            editorEditText.typingAnimationController?.providerUnavailable = true
            if (typingEnabled) {
                editorEditText.setTypingAnimationEnabled(false)
            }
            DiagnosticsLogger.w("WriterSettings", "Failed to inject AnimationEventProvider: typingEnabled=$typingEnabled, providerUnavailable=true, typing animation disabled", e)
        }
    }

    /**
     * 设置自研写作区 SujianEditorView
     */
    private fun setupSelfRenderEditor() {
        DiagnosticsLogger.d("SujianEditor", "Setting up self-render editor (SujianEditorView)")

        // 注入 Core 动画事件提供者
        try {
            val animBridge = com.xiwei.sujian.data.BridgeProvider.getEditorAnimationBridge(requireContext())
            sujianEditorView.setAnimationEventProvider(AnimationEventProvider { oldText, newText, oldCursorIndex, newCursorIndex, cause, maxAnimatedChars, animationDurationMs ->
                try {
                    when (val result = animBridge.editorAnimationEvents(oldText, newText, oldCursorIndex, newCursorIndex, cause, maxAnimatedChars, animationDurationMs)) {
                        is com.xiwei.sujian.data.BridgeResult.Success -> result.data
                        else -> emptyList()
                    }
                } catch (_: Exception) {
                    emptyList()
                }
            })
            DiagnosticsLogger.d("SujianEditor", "AnimationEventProvider injected for SujianEditorView")
        } catch (e: Exception) {
            DiagnosticsLogger.w("SujianEditor", "Failed to inject AnimationEventProvider for SujianEditorView", e)
        }

        // 内容变更监听
        sujianEditorView.onContentChanged = { newText ->
            if (projectId != null && volumeId != null && chapterId != null) {
                viewModel.onContentChanged(newText)
            }
        }
    }

    override fun onResume() {
        super.onResume()
        if (!useSelfRenderEditor && !::editorEditText.isInitialized) return
        if (!useSelfRenderEditor) {
            editorEditText.onEditorResume()
        }

        if (SyncChangeBus.consumeChanged()) {
            val pid = projectId
            val vid = volumeId
            val cid = chapterId
            if (pid != null && vid != null && cid != null) {
                try {
                    workspaceRepository.getChapterContentWithMeta(pid, vid, cid)
                } catch (_: Exception) {
                    Toast.makeText(requireContext(), getString(R.string.error_chapter_deleted_remotely), Toast.LENGTH_LONG).show()
                    callback?.onBackRequested()
                    return
                }
            }
        }
        if (CoreSettingsEvents.consumeEditorChanged()) {
            viewModel.onSettingsChanged()
        }
    }

    // ── Public API ──

    /**
     * 初始化/切换章节。TwoPane 模式下切换章节时调用。
     */
    fun initChapter(projectId: String, volumeId: String, chapterId: String, chapterTitle: String) {
        this.projectId = projectId
        this.volumeId = volumeId
        this.chapterId = chapterId
        viewModel.initChapter(projectId, volumeId, chapterId, chapterTitle)
    }

    /**
     * 请求保存当前内容。返回 Deferred<Boolean>，true 表示保存成功。
     */
    fun requestSave(): Deferred<Boolean> {
        return viewModel.requestSave()
    }

    /**
     * 获取当前章节 ID
     */
    fun getCurrentChapterId(): String? = chapterId

    /**
     * 设置宿主回调
     */
    fun setCallback(callback: EditorFragmentCallback) {
        this.callback = callback
    }

    /**
     * 切换搜索栏可见性（供宿主 toolbar 菜单调用）
     */
    fun toggleSearchBar() {
        if (searchLayout.visibility == View.VISIBLE) {
            searchLayout.visibility = View.GONE
            clearHighlights()
        } else {
            searchLayout.visibility = View.VISIBLE
            etSearch.requestFocus()
            performSearch()
        }
    }

    /**
     * 显示章节备注对话框
     */
    fun showChapterNoteDialog() {
        val pid = projectId
        val vid = volumeId
        val cid = chapterId
        if (pid == null || vid == null || cid == null) return

        val currentNote = viewModel.uiState.value.chapterNote ?: ""

        val editText = EditText(requireContext())
        editText.hint = getString(R.string.hint_chapter_note)
        editText.setPadding(48, 48, 48, 48)
        editText.inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE
        editText.minLines = 3
        editText.maxLines = 10
        editText.gravity = android.view.Gravity.TOP or android.view.Gravity.START
        editText.setText(currentNote)

        AlertDialog.Builder(requireContext())
            .setTitle(R.string.dialog_chapter_note_title)
            .setView(editText)
            .setPositiveButton(R.string.action_save) { _, _ ->
                val newNote = editText.text.toString().trim()
                viewModel.updateChapterNote(newNote)
            }
            .setNegativeButton(R.string.action_cancel, null)
            .show()
    }

    // ── Options Menu ──

    override fun onCreateOptionsMenu(menu: Menu, inflater: MenuInflater) {
        inflater.inflate(R.menu.menu_editor, menu)
        super.onCreateOptionsMenu(menu, inflater)
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
                val intent = android.content.Intent(requireContext(), SettingsActivity::class.java)
                startActivity(intent)
                true
            }
            R.id.action_registry -> {
                val intent = android.content.Intent(requireContext(), ActionRegistryActivity::class.java)
                startActivity(intent)
                true
            }
            else -> super.onOptionsItemSelected(item)
        }
    }

    // ── ViewModel Observation ──

    private fun observeViewModel() {
        viewLifecycleOwner.lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                viewModel.uiState.collectLatest { state ->
                    if (state.loading) {
                        tvSaveStatus.text = ""
                        return@collectLatest
                    }

                    if (useSelfRenderEditor) {
                        // 自研写作区：加载内容
                        if (!sujianEditorView.hasFocus()) {
                            if (sujianEditorView.getText() != state.content) {
                                sujianEditorView.setText(state.content)
                            }
                        }
                    } else {
                        // 旧版编辑器：加载内容
                        if (!editorEditText.hasFocus()) {
                            editorEditText.runWithoutTextAnimations {
                                if (editorEditText.text?.toString() != state.content) {
                                    editorEditText.setText(state.content)
                                }
                            }
                        }
                    }

                    if (useSelfRenderEditor) {
                        // 自研写作区没有 isEnabled 概念，暂不处理
                    } else {
                        editorEditText.isEnabled = state.editorEnabled
                    }

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

        viewLifecycleOwner.lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                viewModel.events.collectLatest { event ->
                    when (event) {
                        is EditorEvent.ToastMessage -> {
                            Toast.makeText(requireContext(), event.message, Toast.LENGTH_LONG).show()
                        }
                        is EditorEvent.ShowSaveFailedDialog -> {
                            showSaveFailedDialog(event.message)
                        }
                    }
                }
            }
        }
    }

    private fun applySettingsToEditor(settings: EditorSettingsState) {
        val tag = "WriterSettings"
        DiagnosticsLogger.d(tag, "applySettingsToEditor: fontSize=${settings.fontSize}, lineSpacing=${settings.lineSpacingMultiplier}, " +
            "autoIndent=${settings.autoIndentEnabled}/${settings.autoIndentWidth}, " +
            "typingAnim=${settings.typingAnimationEnabled}/${settings.typingAnimationDurationMs}ms, " +
            "smoothCursor=${settings.smoothCursorEnabled}/${settings.smoothCursorDurationMs}ms, " +
            "selfRender=$useSelfRenderEditor")

        if (useSelfRenderEditor) {
            // ── 自研写作区设置 ──
            if (lastFontSize != settings.fontSize) {
                lastFontSize = settings.fontSize
                sujianEditorView.setFontSize(settings.fontSize)
                DiagnosticsLogger.d(tag, "  → fontSize applied to SujianEditorView: ${settings.fontSize}")
            }
            if (lastLineSpacing != settings.lineSpacingMultiplier) {
                lastLineSpacing = settings.lineSpacingMultiplier
                sujianEditorView.setLineSpacingMultiplier(settings.lineSpacingMultiplier)
                DiagnosticsLogger.d(tag, "  → lineSpacing applied to SujianEditorView: ${settings.lineSpacingMultiplier}")
            }
            if (lastTypingAnimEnabled != settings.typingAnimationEnabled || lastTypingAnimDuration != settings.typingAnimationDurationMs) {
                lastTypingAnimEnabled = settings.typingAnimationEnabled
                lastTypingAnimDuration = settings.typingAnimationDurationMs
                sujianEditorView.setTypingAnimationEnabled(settings.typingAnimationEnabled, settings.typingAnimationDurationMs)
                DiagnosticsLogger.d(tag, "  → typingAnimation applied to SujianEditorView: ${settings.typingAnimationEnabled}/${settings.typingAnimationDurationMs}ms")
            }
            if (lastSmoothCursorEnabled != settings.smoothCursorEnabled || lastSmoothCursorDuration != settings.smoothCursorDurationMs) {
                lastSmoothCursorEnabled = settings.smoothCursorEnabled
                lastSmoothCursorDuration = settings.smoothCursorDurationMs
                sujianEditorView.setSmoothCursorEnabled(settings.smoothCursorEnabled, settings.smoothCursorDurationMs)
                DiagnosticsLogger.d(tag, "  → smoothCursor applied to SujianEditorView: ${settings.smoothCursorEnabled}/${settings.smoothCursorDurationMs}ms")
            }
            DiagnosticsLogger.d(tag, "applySettingsToEditor: SujianEditorView settings applied, typingAnim=${settings.typingAnimationEnabled}, smoothCursor=${settings.smoothCursorEnabled}")
        } else {
            // ── 旧版编辑器设置 ──
            editorEditText.runWithoutTextAnimations {
                if (lastFontSize != settings.fontSize) {
                    lastFontSize = settings.fontSize
                    editorEditText.textSize = settings.fontSize
                    DiagnosticsLogger.d(tag, "  → fontSize applied: ${settings.fontSize}")
                }
                if (lastLineSpacing != settings.lineSpacingMultiplier) {
                    lastLineSpacing = settings.lineSpacingMultiplier
                    editorEditText.setLineSpacing(0f, settings.lineSpacingMultiplier)
                    DiagnosticsLogger.d(tag, "  → lineSpacing applied: ${settings.lineSpacingMultiplier}")
                }
            }

            editorEditText.setAutoIndent(settings.autoIndentEnabled, settings.autoIndentWidth)
            if (lastTypingAnimEnabled != settings.typingAnimationEnabled || lastTypingAnimDuration != settings.typingAnimationDurationMs) {
                lastTypingAnimEnabled = settings.typingAnimationEnabled
                lastTypingAnimDuration = settings.typingAnimationDurationMs
                editorEditText.setTypingAnimationEnabled(settings.typingAnimationEnabled, settings.typingAnimationDurationMs)
                DiagnosticsLogger.d(tag, "  → typingAnimation applied: ${settings.typingAnimationEnabled}/${settings.typingAnimationDurationMs}ms")
            }
            if (lastSmoothCursorEnabled != settings.smoothCursorEnabled || lastSmoothCursorDuration != settings.smoothCursorDurationMs) {
                lastSmoothCursorEnabled = settings.smoothCursorEnabled
                lastSmoothCursorDuration = settings.smoothCursorDurationMs
                editorEditText.setSmoothCursorEnabled(settings.smoothCursorEnabled, settings.smoothCursorDurationMs)
                DiagnosticsLogger.d(tag, "  → smoothCursor applied: ${settings.smoothCursorEnabled}/${settings.smoothCursorDurationMs}ms")
            }

            val typingCtrl = editorEditText.typingAnimationController
            val animActualPath = when {
                !settings.typingAnimationEnabled -> "disabled"
                typingCtrl?.hasProvider == true && !typingCtrl.providerFailedLastTime -> "core"
                typingCtrl?.hasProvider == true && typingCtrl.providerFailedLastTime -> "core(failed,skip)"
                typingCtrl?.providerUnavailable == true -> "no-provider(disabled)"
                else -> "no-provider(disabled)"
            }
            DiagnosticsLogger.d(tag, "applySettingsToEditor: all settings applied, settingEnabled=${settings.typingAnimationEnabled}, providerAvailable=${typingCtrl?.hasProvider == true}, actualAnimationPath=$animActualPath, smoothCursor=${settings.smoothCursorEnabled}")
        }
    }

    // ── Text Watcher ──

    private fun setupTextWatcher() {
        textWatcher = object : android.text.TextWatcher {
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

    // ── Save Failed Dialog ──

    private fun showSaveFailedDialog(message: String) {
        AlertDialog.Builder(requireContext())
            .setTitle(R.string.dialog_save_failed_title)
            .setMessage(message)
            .setPositiveButton(R.string.action_exit) { _, _ ->
                callback?.onBackRequested()
            }
            .setNegativeButton(R.string.action_retry) { _, _ ->
                lifecycleScope.launch {
                    val success = viewModel.requestSave().await()
                    if (success) {
                        callback?.onBackRequested()
                    }
                }
            }
            .show()
    }

    // ── Search and Replace ──

    private fun setupSearchAndReplace() {
        btnSearchClose.setOnClickListener {
            searchLayout.visibility = View.GONE
            clearHighlights()
        }

        etSearch.addTextChangedListener(object : android.text.TextWatcher {
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

    private fun performSearch() {
        clearHighlights()
        searchResults.clear()
        currentSearchIndex = -1

        if (searchLayout.visibility == View.GONE) return

        val searchStr = etSearch.text.toString()
        if (searchStr.isEmpty()) return

        val content = editorEditText.text?.toString() ?: return
        var startIndex = content.indexOf(searchStr)
        val highlightColor = requireContext().getColor(com.google.android.material.R.color.material_dynamic_primary70)

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
            val highlightColor = requireContext().getColor(com.google.android.material.R.color.material_dynamic_primary70)
            val activeColor = requireContext().getColor(com.google.android.material.R.color.material_dynamic_primary50)

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