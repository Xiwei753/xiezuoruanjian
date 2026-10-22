package com.xiwei.sujian.feature.search.data

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import com.xiwei.sujian.feature.search.data.model.SearchContext
import com.xiwei.sujian.feature.search.data.model.SearchHit
import com.xiwei.sujian.feature.search.data.model.SearchPage
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

data class SearchUiState(
    val query: String = "",
    val results: List<SearchHit> = emptyList(),
    val isLoading: Boolean = false,
    val isLoadingMore: Boolean = false,
    val hasMore: Boolean = false,
    val hasSearched: Boolean = false,
    val error: String? = null,
    val missingObjectIds: Set<String> = emptySet(),
)

/**
 * 搜索控制器 — 持有查询输入、翻页游标与结果状态。
 *
 * - 输入防抖，且每次输入递增请求序号；旧请求即使取消不及时也不能覆盖新结果；
 * - Core 查询固定在 [Dispatchers.IO]，主线程只接收最终状态；
 * - 翻页只追加当前查询代次的结果，查询变化后旧游标立即失效。
 */
class SearchController(
    private val repository: SearchRepository,
    private val scope: String,
    private val searchContext: SearchContext,
    private val debounceMillis: Long = DEFAULT_DEBOUNCE_MILLIS,
) : ViewModel() {
    private val _state = MutableStateFlow(SearchUiState())
    val state: StateFlow<SearchUiState> = _state.asStateFlow()

    private var searchJob: Job? = null
    private var nextCursor: String? = null
    private var requestGeneration: Long = 0L

    fun onQueryChanged(rawQuery: String) {
        val query = rawQuery.trim()
        _state.update {
            it.copy(
                query = rawQuery,
                results = emptyList(),
                isLoading = query.isNotEmpty(),
                isLoadingMore = false,
                hasMore = false,
                hasSearched = false,
                error = null,
                missingObjectIds = emptySet(),
            )
        }
        searchJob?.cancel()
        nextCursor = null
        val generation = ++requestGeneration
        if (query.isEmpty()) {
            return
        }
        searchJob =
            viewModelScope.launch {
                delay(debounceMillis)
                executeSearch(query, generation, cursor = null, append = false)
            }
    }

    /** 键盘搜索键/重试 — 跳过防抖立即查询当前输入。 */
    fun searchNow() {
        searchJob?.cancel()
        nextCursor = null
        val query = _state.value.query.trim()
        val generation = ++requestGeneration
        if (query.isEmpty()) return
        searchJob =
            viewModelScope.launch {
                executeSearch(query, generation, cursor = null, append = false)
            }
    }

    fun loadMore() {
        val cursor = nextCursor ?: return
        val current = _state.value
        if (current.isLoading || current.isLoadingMore) return
        if (searchJob?.isActive == true) return
        val query = current.query.trim()
        if (query.isEmpty()) return
        val generation = requestGeneration
        searchJob =
            viewModelScope.launch {
                executeSearch(query, generation, cursor = cursor, append = true)
            }
    }

    /** 结果目标已被删除 — 当前条目显示「内容已不存在」，不再提供跳转。 */
    fun markTargetMissing(objectId: String) {
        if (objectId.isEmpty()) return
        _state.update { it.copy(missingObjectIds = it.missingObjectIds + objectId) }
    }

    private suspend fun executeSearch(
        query: String,
        generation: Long,
        cursor: String?,
        append: Boolean,
    ) {
        _state.update {
            if (append) {
                it.copy(isLoadingMore = true, error = null)
            } else {
                it.copy(isLoading = true, isLoadingMore = false, error = null)
            }
        }
        try {
            val page =
                withContext(Dispatchers.IO) {
                    repository.searchPage(
                        query = query,
                        scope = scope,
                        limit = PAGE_SIZE,
                        cursor = cursor,
                        searchContext = searchContext,
                    )
                }
            applyPage(query, generation, page, append)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            applyFailure(query, generation, e.message.orEmpty(), append)
        }
    }

    private fun applyPage(
        query: String,
        generation: Long,
        page: SearchPage,
        append: Boolean,
    ) {
        if (!isCurrentRequest(query, generation)) return
        nextCursor = page.nextCursor
        _state.update { previous ->
            val merged =
                if (append) {
                    (previous.results + page.items).distinctBy { it.stableKey }
                } else {
                    page.items
                }
            previous.copy(
                results = merged,
                isLoading = false,
                isLoadingMore = false,
                hasMore = page.nextCursor != null,
                hasSearched = true,
                error = null,
            )
        }
    }

    private fun applyFailure(
        query: String,
        generation: Long,
        message: String,
        append: Boolean,
    ) {
        if (!isCurrentRequest(query, generation)) return
        _state.update {
            it.copy(
                isLoading = false,
                isLoadingMore = false,
                hasMore = if (append) it.hasMore else false,
                hasSearched = true,
                error = message,
            )
        }
    }

    private fun isCurrentRequest(
        query: String,
        generation: Long,
    ): Boolean = generation == requestGeneration && _state.value.query.trim() == query

    class Factory(
        private val repository: SearchRepository,
        private val scope: String,
        private val searchContext: SearchContext,
    ) : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T =
            SearchController(repository, scope, searchContext) as T
    }

    private companion object {
        const val PAGE_SIZE = 30
        const val DEFAULT_DEBOUNCE_MILLIS = 250L
    }
}
