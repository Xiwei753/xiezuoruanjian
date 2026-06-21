package com.xiwei.sujian.ui

import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.recyclerview.widget.RecyclerView
import com.xiwei.sujian.R

/**
 * 章节列表项数据类（用于双栏模式右侧面板的章节列表）
 *
 * 与 ChapterListActivity.ListItem 结构对齐，但独立定义以解耦。
 */
sealed class ChapterListItem {
    data class VolumeHeader(val volumeId: String, val title: String) : ChapterListItem()
    data class ChapterItem(val volumeId: String, val chapterId: String, val title: String, val wordCount: Int) : ChapterListItem()
    data class EmptyHint(val volumeId: String) : ChapterListItem()
}

/**
 * DetailChapterAdapter — 双栏模式右侧面板的章节列表 Adapter
 *
 * 使用 XML item layout + Material theme token 颜色，
 * 不再硬编码 android.R.color.black / android.R.color.darker_gray。
 */
class DetailChapterAdapter(
    private val items: List<ChapterListItem>,
    private val onChapterClick: (volumeId: String, chapterId: String, chapterTitle: String) -> Unit
) : RecyclerView.Adapter<DetailChapterAdapter.ViewHolder>() {

    companion object {
        private const val TYPE_VOLUME_HEADER = 0
        private const val TYPE_CHAPTER = 1
        private const val TYPE_EMPTY_HINT = 2
    }

    override fun getItemViewType(position: Int): Int = when (items[position]) {
        is ChapterListItem.VolumeHeader -> TYPE_VOLUME_HEADER
        is ChapterListItem.ChapterItem -> TYPE_CHAPTER
        is ChapterListItem.EmptyHint -> TYPE_EMPTY_HINT
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_detail_chapter, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        when (val item = items[position]) {
            is ChapterListItem.VolumeHeader -> {
                holder.volumeTitle.text = "📖 ${item.title}"
                holder.volumeTitle.visibility = View.VISIBLE
                holder.chapterTitle.visibility = View.GONE
                holder.emptyHint.visibility = View.GONE
                holder.volumeTitle.setOnClickListener(null)
            }
            is ChapterListItem.ChapterItem -> {
                holder.volumeTitle.visibility = View.GONE
                holder.chapterTitle.text = parent.context.getString(R.string.chapter_title_with_count, item.title, item.wordCount)
                holder.chapterTitle.visibility = View.VISIBLE
                holder.emptyHint.visibility = View.GONE
                holder.chapterTitle.setOnClickListener {
                    onChapterClick(item.volumeId, item.chapterId, item.title)
                }
            }
            is ChapterListItem.EmptyHint -> {
                holder.volumeTitle.visibility = View.GONE
                holder.chapterTitle.visibility = View.GONE
                holder.emptyHint.text = parent.context.getString(R.string.empty_volume_hint)
                holder.emptyHint.visibility = View.VISIBLE
                holder.emptyHint.setOnClickListener(null)
            }
        }
    }

    override fun getItemCount() = items.size

    class ViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
        val volumeTitle: TextView = itemView.findViewById(R.id.tvVolumeTitle)
        val chapterTitle: TextView = itemView.findViewById(R.id.tvChapterTitle)
        val emptyHint: TextView = itemView.findViewById(R.id.tvEmptyHint)
    }
}