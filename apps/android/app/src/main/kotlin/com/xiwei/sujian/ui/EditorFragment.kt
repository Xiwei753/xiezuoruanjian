package com.xiwei.sujian.ui

import android.os.Bundle

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
import com.xiwei.sujian.data.BridgeProvider
import com.xiwei.sujian.editor.v2.coordinator.AnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.host.SujianEditorView
import com.xiwei.sujian.editor.v2.host.TextEditSessionBridge
import com.xiwei.sujian.editor.v2.compose.LocalAnimatedTextEditorCoordinator
import com.xiwei.sujian.editor.v2.compose.AnimatedTextEditorSlot
import com.xiwei.sujian.ui.compose.editor.WritingPane
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.ui.Modifier
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch

class EditorFragment : Fragment() {

    companion object {
        const val ARG_PROJECT_ID = "PROJECT_ID"
        const val ARG_VOLUME_ID = "VOLUME_ID"
        const val ARG_CHAPTER_ID = "CHAPTER_ID"
        const val ARG_CHAPTER_TITLE = "CHAPTER_TITLE"

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

    interface EditorFragmentCallback {
        fun onBackRequested()
    }

    private lateinit var tvWordCount: TextView
    private lateinit var tvSessionAdded: TextView
    private lateinit var tvSpeed: TextView
    private lateinit var tvSaveStatus: TextView

    private lateinit var searchLayout: LinearLayout
    private lateinit var etSearch: EditText
    private lateinit var etReplace: EditText
    private lateinit var btnSearchNext: ImageButton
    private lateinit var btnSearchClose: ImageButton
    private lateinit var btnReplace: Button
    private lateinit var btnReplaceAll: Button

    private val viewModel: EditorViewModel by viewModels()
    private var projectId: String? = null
    private var volumeId: String? = null
    private var chapterId: String? = null
    private lateinit var workspaceRepository: WorkspaceRepository

    private var searchResults = mutableListOf<Pair<Int, Int>>()
    private var currentSearchIndex = -1

    private var callback: EditorFragmentCallback? = null

    private var lastFontSize: Float? = null
    private var lastLineSpacing: Float? = null
    private var lastTypingAnimEnabled: Boolean? = null
    private var lastTypingAnimDuration: Long? = null
    private var lastSmoothCursorEnabled: Boolean? = null
    private var lastSmoothCursorDuration: Long? = null
    private var lastAutoIndentEnabled: Boolean? = null
    private var lastAutoIndentWidth: Float? = null
    private var lastCoordinatedAnimEnabled: Boolean? = null

    private val coordinator by lazy {
        val bridge = BridgeProvider.getAppServiceBridge(requireContext())
        AnimatedTextEditorCoordinator(requireContext(), bridge)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setHasOptionsMenu(true)
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

        setupComposeEditor(view)

        view.post {
            UiFontUtil.applySansSerifFallback(view)
        }

        val editorStatusBar = view.findViewById<View>(R.id.editorStatusBar)
        val hostController = (activity as? EditorActivity)?.systemBarsController
        if (hostController != null) {
            hostController.addBottomMarginTarget(editorStatusBar)
        } else {
            var originalBottomMargin = 0
            val params = editorStatusBar.layoutParams
            if (params is android.view.ViewGroup.MarginLayoutParams) {
                originalBottomMargin = params.bottomMargin
            }
            ViewCompat.setOnApplyWindowInsetsListener(view) { v, insets ->
                val imeInsets = insets.getInsets(WindowInsetsCompat.Type.ime())
                val systemBarsInsets = insets.getInsets(WindowInsetsCompat.Type.systemBars())
                val bottomInset = maxOf(imeInsets.bottom, systemBarsInsets.bottom)
                val lp = editorStatusBar.layoutParams
                if (lp is android.view.ViewGroup.MarginLayoutParams) {
                    lp.bottomMargin = originalBottomMargin + bottomInset
                    editorStatusBar.layoutParams = lp
                }
                WindowInsetsCompat.CONSUMED
            }
        }

        setupSearchAndReplace()
        observeViewModel()

        val chapterTitle = arguments?.getString(ARG_CHAPTER_TITLE) ?: ""
        if (projectId != null && volumeId != null && chapterId != null) {
            viewModel.initChapter(projectId!!, volumeId!!, chapterId!!, chapterTitle)
        } else {
            viewModel.initErrorState(getString(R.string.error_missing_chapter_identifiers))
        }
    }

    private fun setupComposeEditor(view: View) {
        val container = view.findViewById<ViewGroup>(R.id.editorComposeContainer) ?: return
        val composeView = androidx.compose.ui.platform.ComposeView(requireContext()).apply {
            setViewCompositionStrategy(androidx.compose.ui.platform.ViewCompositionStrategy.DisposeOnViewTreeLifecycleDestroyed)
        }
        container.addView(composeView, ViewGroup.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.MATCH_PARENT
        ))

        val pid = projectId ?: ""
        val vid = volumeId ?: ""
        val cid = chapterId ?: ""
        val title = arguments?.getString(ARG_CHAPTER_TITLE) ?: ""

        composeView.setContent {
            androidx.compose.runtime.CompositionLocalProvider(
                LocalAnimatedTextEditorCoordinator provides coordinator
            ) {
                Box(
                    modifier = Modifier.fillMaxSize()
                ) {
                    WritingPane(
                        projectId = pid,
                        volumeId = vid,
                        chapterId = cid,
                        chapterTitle = title,
                        modifier = Modifier.fillMaxSize()
                    )
                    AnimatedTextEditorSlot(
                        coordinator = coordinator,
                        modifier = Modifier.fillMaxSize()
                    )
                }
            }
        }
    }

    private fun getEditorView(): SujianEditorView? = coordinator.getSharedEditorView()

    override fun onResume() {
        super.onResume()

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

    fun initChapter(projectId: String, volumeId: String, chapterId: String, chapterTitle: String) {
        this.projectId = projectId
        this.volumeId = volumeId
        this.chapterId = chapterId
        viewModel.initChapter(projectId, volumeId, chapterId, chapterTitle)
    }

    fun requestSave(): Deferred<Boolean> {
        return viewModel.requestSave()
    }

    fun clearChapterContent() {
        viewModel.clearChapterContent()
    }

    fun getCurrentChapterId(): String? = chapterId

    fun setCallback(callback: EditorFragmentCallback) {
        this.callback = callback
    }

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

    private fun observeViewModel() {
        viewLifecycleOwner.lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                viewModel.uiState.collectLatest { state ->
                    if (state.loading) {
                        tvSaveStatus.text = ""
                        return@collectLatest
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
        val view = getEditorView() ?: return

        if (lastFontSize != settings.fontSize) {
            lastFontSize = settings.fontSize
            view.setFontSize(settings.fontSize)
        }
        if (lastLineSpacing != settings.lineSpacingMultiplier) {
            lastLineSpacing = settings.lineSpacingMultiplier
            view.setLineSpacingMultiplier(settings.lineSpacingMultiplier)
        }
        if (lastTypingAnimEnabled != settings.typingAnimationEnabled || lastTypingAnimDuration != settings.typingAnimationDurationMs) {
            lastTypingAnimEnabled = settings.typingAnimationEnabled
            lastTypingAnimDuration = settings.typingAnimationDurationMs
            view.setTypingAnimationEnabled(settings.typingAnimationEnabled, settings.typingAnimationDurationMs)
        }
        if (lastSmoothCursorEnabled != settings.smoothCursorEnabled || lastSmoothCursorDuration != settings.smoothCursorDurationMs) {
            lastSmoothCursorEnabled = settings.smoothCursorEnabled
            lastSmoothCursorDuration = settings.smoothCursorDurationMs
            view.setSmoothCursorEnabled(settings.smoothCursorEnabled, settings.smoothCursorDurationMs)
        }
        if (lastAutoIndentEnabled != settings.autoIndentEnabled || lastAutoIndentWidth != settings.autoIndentWidth) {
            lastAutoIndentEnabled = settings.autoIndentEnabled
            lastAutoIndentWidth = settings.autoIndentWidth
            view.setAutoIndent(settings.autoIndentEnabled, settings.autoIndentWidth)
        }
        if (lastCoordinatedAnimEnabled != settings.coordinatedTextCursorAnimationEnabled) {
            lastCoordinatedAnimEnabled = settings.coordinatedTextCursorAnimationEnabled
            view.setCoordinatedAnimationEnabled(settings.coordinatedTextCursorAnimationEnabled)
        }
    }

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
            override fun afterTextChanged(s: android.text.Editable?) {}
        })

        btnSearchNext.setOnClickListener {
            if (searchResults.isNotEmpty()) {
                currentSearchIndex = (currentSearchIndex + 1) % searchResults.size
                focusSearchResult()
            }
        }

        btnReplace.setOnClickListener {
            val view = getEditorView() ?: return@setOnClickListener
            if (searchResults.isEmpty() || currentSearchIndex < 0) return@setOnClickListener
            val (start, end) = searchResults[currentSearchIndex]
            val replaceStr = etReplace.text.toString()
            view.replaceRange(start, end, replaceStr)
            performSearch()
        }

        btnReplaceAll.setOnClickListener {
            val view = getEditorView() ?: return@setOnClickListener
            if (searchResults.isEmpty()) return@setOnClickListener
            val searchStr = etSearch.text.toString()
            val replaceStr = etReplace.text.toString()
            view.replaceAll(searchStr, replaceStr)
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

        val view = getEditorView() ?: return
        val content = view.getText()
        var startIndex = content.indexOf(searchStr)

        while (startIndex >= 0) {
            val endIndex = startIndex + searchStr.length
            searchResults.add(Pair(startIndex, endIndex))
            startIndex = content.indexOf(searchStr, endIndex)
        }

        if (searchResults.isNotEmpty()) {
            currentSearchIndex = 0
            view.setSearchHighlights(searchResults)
            focusSearchResult()
        }
    }

    private fun focusSearchResult() {
        val view = getEditorView() ?: return
        if (searchResults.isEmpty() || currentSearchIndex < 0) return
        val (start, end) = searchResults[currentSearchIndex]
        view.setSelectionRange(start, end)
        view.scrollToSelection()
    }

    private fun clearHighlights() {
        getEditorView()?.clearSearchHighlights()
        searchResults.clear()
        currentSearchIndex = -1
    }
}
