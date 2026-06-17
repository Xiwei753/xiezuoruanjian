package com.xiwei.sujian.ui

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.fragment.app.Fragment
import androidx.lifecycle.lifecycleScope
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.xiwei.sujian.R
import com.xiwei.sujian.data.WorkspaceRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * DetailChapterListFragment — TwoPane 模式下右侧面板的章节列表
 *
 * 显示指定项目的卷/章结构，点击章节时通知宿主 Activity。
 * 使用 Material theme token 颜色，共享 item layout。
 */
class DetailChapterListFragment : Fragment() {

    private var projectId: String = ""
    private var projectTitle: String = ""
    private var onChapterClickListener: ((volumeId: String, chapterId: String, chapterTitle: String) -> Unit)? = null

    companion object {
        private const val ARG_PROJECT_ID = "projectId"
        private const val ARG_PROJECT_TITLE = "projectTitle"

        fun newInstance(projectId: String, projectTitle: String): DetailChapterListFragment {
            return DetailChapterListFragment().apply {
                arguments = Bundle().apply {
                    putString(ARG_PROJECT_ID, projectId)
                    putString(ARG_PROJECT_TITLE, projectTitle)
                }
            }
        }
    }

    fun setOnChapterClickListener(listener: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit) {
        onChapterClickListener = listener
    }

    override fun onCreateView(inflater: LayoutInflater, container: ViewGroup?, savedInstanceState: Bundle?): View {
        projectId = arguments?.getString(ARG_PROJECT_ID) ?: ""
        projectTitle = arguments?.getString(ARG_PROJECT_TITLE) ?: ""

        val root = inflater.inflate(R.layout.fragment_detail_chapter_list, container, false)

        val titleText: TextView = root.findViewById(R.id.tvDetailTitle)
        titleText.text = projectTitle

        val recyclerView: RecyclerView = root.findViewById(R.id.detailChapterRecyclerView)
        recyclerView.layoutManager = LinearLayoutManager(requireContext())

        loadChapters(recyclerView)

        return root
    }

    private fun loadChapters(recyclerView: RecyclerView) {
        lifecycleScope.launch {
            val items = ErrorUtil.safeRunSuspend(requireActivity(), emptyList<ChapterListItem>()) {
                withContext(Dispatchers.IO) {
                    val workspaceRepository = WorkspaceRepository(requireActivity())
                    val volumes = workspaceRepository.getVolumes(projectId)
                    val result = mutableListOf<ChapterListItem>()
                    for (volume in volumes) {
                        result.add(ChapterListItem.VolumeHeader(volume.id, volume.title))
                        val chapters = workspaceRepository.getChapters(projectId, volume.id)
                        if (chapters.isEmpty()) {
                            result.add(ChapterListItem.EmptyHint(volume.id))
                        } else {
                            for (chapter in chapters) {
                                result.add(ChapterListItem.ChapterItem(volume.id, chapter.id, chapter.title, chapter.wordCount))
                            }
                        }
                    }
                    result
                }
            }

            val adapter = DetailChapterAdapter(items) { volumeId, chapterId, chapterTitle ->
                onChapterClickListener?.invoke(volumeId, chapterId, chapterTitle)
            }
            recyclerView.adapter = adapter
        }
    }
}