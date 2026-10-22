package com.xiwei.sujian.feature.search.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.xiwei.sujian.R
import com.xiwei.sujian.app.di.LocalSujianAppDependencies
import com.xiwei.sujian.core.designsystem.icon.SujianIcons
import com.xiwei.sujian.core.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.core.designsystem.theme.LocalSujianDimensions
import com.xiwei.sujian.feature.search.data.SearchController
import com.xiwei.sujian.feature.search.data.SearchUiState
import com.xiwei.sujian.feature.search.data.model.SearchContext
import com.xiwei.sujian.feature.search.data.model.SearchHit
import kotlinx.coroutines.launch

/**
 * 全局搜索页 — Material3 输入框 + LazyColumn 结果列表。
 *
 * 同章节的多个正文命中是独立条目，不按章节聚合。
 * 点击结果由调用方负责跳转；返回 false 表示目标已不存在，当前条目进入失效态。
 */
@Composable
fun SearchScreen(
    searchContext: SearchContext,
    scope: String,
    onOpenResult: suspend (SearchHit) -> Boolean,
    modifier: Modifier = Modifier,
) {
    val deps = LocalSujianAppDependencies.current
    val context = LocalContext.current
    val controller: SearchController =
        viewModel(
            factory =
                SearchController.Factory(
                    repository = deps.searchRepository,
                    scope = scope,
                    searchContext = searchContext,
                ),
        )
    val state by controller.state.collectAsStateWithLifecycle()
    val listState = rememberLazyListState()
    val coroutineScope = rememberCoroutineScope()
    val focusRequester = remember { FocusRequester() }
    val dims = LocalSujianDimensions.current

    LaunchedEffect(Unit) {
        focusRequester.requestFocus()
    }
    LaunchedEffect(state.query) {
        if (state.results.isNotEmpty()) listState.scrollToItem(0)
    }

    Column(
        modifier =
            modifier
                .fillMaxSize()
                .testTag(SujianSemanticIds.SearchScreen),
    ) {
        SearchInputField(
            state = state,
            focusRequester = focusRequester,
            hintResId = R.string.search_input_hint,
            onQueryChanged = controller::onQueryChanged,
            onSearch = controller::searchNow,
        )

        if (state.isLoading && state.results.isNotEmpty()) {
            LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
        }

        Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
            SearchContent(
                state = state,
                listState = listState,
                dims = dims,
                onRetry = controller::searchNow,
                onLoadMore = controller::loadMore,
                onOpenResult = { hit ->
                    coroutineScope.launch {
                        val opened = onOpenResult(hit)
                        if (!opened) controller.markTargetMissing(hit.objectId)
                    }
                },
            )
        }
    }
}

@Composable
private fun SearchInputField(
    state: SearchUiState,
    focusRequester: FocusRequester,
    hintResId: Int,
    onQueryChanged: (String) -> Unit,
    onSearch: () -> Unit,
) {
    val dims = LocalSujianDimensions.current
    OutlinedTextField(
        value = state.query,
        onValueChange = onQueryChanged,
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(horizontal = dims.space16, vertical = dims.space8)
                .focusRequester(focusRequester)
                .testTag(SujianSemanticIds.SearchInput),
        placeholder = { Text(stringResource(hintResId)) },
        leadingIcon = {
            Icon(
                imageVector = SujianIcons.Search,
                contentDescription = null,
            )
        },
        trailingIcon = {
            if (state.query.isNotEmpty()) {
                IconButton(onClick = { onQueryChanged("") }) {
                    Icon(
                        imageVector = SujianIcons.Close,
                        contentDescription = stringResource(R.string.search_clear),
                    )
                }
            }
        },
        singleLine = true,
        shape = MaterialTheme.shapes.extraLarge,
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
        keyboardActions = KeyboardActions(onSearch = { onSearch() }),
    )
}

@Composable
private fun SearchContent(
    state: SearchUiState,
    listState: androidx.compose.foundation.lazy.LazyListState,
    dims: com.xiwei.sujian.core.designsystem.theme.SujianDimensions,
    onRetry: () -> Unit,
    onLoadMore: () -> Unit,
    onOpenResult: (SearchHit) -> Unit,
) {
    when {
        state.query.isBlank() -> {
            SearchMessage(
                title = stringResource(R.string.search_prompt_title),
                supporting = stringResource(R.string.search_prompt_supporting),
            )
        }
        state.isLoading && state.results.isEmpty() -> {
            Box(
                modifier = Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                CircularProgressIndicator()
            }
        }
        state.error != null && state.results.isEmpty() -> {
            SearchMessage(
                title = stringResource(R.string.search_failed_title),
                supporting = state.error,
                actionLabel = stringResource(R.string.search_retry),
                onAction = onRetry,
            )
        }
        state.hasSearched && state.results.isEmpty() -> {
            SearchMessage(
                title = stringResource(R.string.search_empty_title),
                supporting = stringResource(R.string.search_empty_supporting),
            )
        }
        else -> {
            SearchResultList(
                state = state,
                listState = listState,
                dims = dims,
                onLoadMore = onLoadMore,
                onOpenResult = onOpenResult,
            )
        }
    }
}

@Composable
private fun SearchResultList(
    state: SearchUiState,
    listState: androidx.compose.foundation.lazy.LazyListState,
    dims: com.xiwei.sujian.core.designsystem.theme.SujianDimensions,
    onLoadMore: () -> Unit,
    onOpenResult: (SearchHit) -> Unit,
) {
    val highlightColor = MaterialTheme.colorScheme.primaryContainer
    LazyColumn(
        state = listState,
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(horizontal = dims.space16, vertical = dims.space8),
        verticalArrangement = Arrangement.spacedBy(dims.space8),
    ) {
        if (state.error != null) {
            item(key = "search-error") {
                Text(
                    text = stringResource(R.string.search_failed_inline, state.error),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
        items(
            items = state.results,
            key = { it.stableKey },
        ) { hit ->
            SearchResultRow(
                hit = hit,
                missing = hit.objectId in state.missingObjectIds,
                highlightColor = highlightColor,
                onClick = { onOpenResult(hit) },
            )
        }
        if (state.hasMore) {
            item(key = "search-load-more") {
                TextButton(
                    onClick = onLoadMore,
                    enabled = !state.isLoadingMore,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(
                        text =
                            if (state.isLoadingMore) {
                                stringResource(R.string.search_loading_more)
                            } else {
                                stringResource(R.string.search_load_more)
                            },
                    )
                }
            }
        }
    }
}

@Composable
private fun SearchResultRow(
    hit: SearchHit,
    missing: Boolean,
    highlightColor: androidx.compose.ui.graphics.Color,
    onClick: () -> Unit,
) {
    val dims = LocalSujianDimensions.current
    Surface(
        modifier =
            Modifier
                .fillMaxWidth()
                .clickable(enabled = !missing, onClick = onClick),
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surfaceContainerLow,
    ) {
        Column(modifier = Modifier.padding(horizontal = dims.space16, vertical = dims.space12)) {
            Text(
                text = hit.path,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(2.dp))
            SearchResultTitle(hit = hit, missing = missing, highlightColor = highlightColor)
            SearchResultSummary(hit = hit, missing = missing, highlightColor = highlightColor)
            if (missing) {
                Spacer(Modifier.height(4.dp))
                Text(
                    text = stringResource(R.string.search_result_missing),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.error,
                )
            }
        }
    }
}

@Composable
private fun SearchResultTitle(
    hit: SearchHit,
    missing: Boolean,
    highlightColor: androidx.compose.ui.graphics.Color,
) {
    val titleText =
        remember(hit.title, hit.summaryMatchRanges, hit.field, highlightColor) {
            if (hit.isTitleHit) {
                highlightedSearchText(hit.title, hit.summaryMatchRanges, highlightColor)
            } else {
                androidx.compose.ui.text.AnnotatedString(hit.title)
            }
        }
    Text(
        text = titleText,
        style = MaterialTheme.typography.titleMedium,
        color =
            if (missing) {
                MaterialTheme.colorScheme.onSurfaceVariant
            } else {
                MaterialTheme.colorScheme.onSurface
            },
    )
}

@Composable
private fun SearchResultSummary(
    hit: SearchHit,
    missing: Boolean,
    highlightColor: androidx.compose.ui.graphics.Color,
) {
    if (hit.summary.isBlank() || hit.summary == hit.title) return
    val summaryText =
        remember(hit.summary, hit.summaryMatchRanges, highlightColor) {
            highlightedSearchText(hit.summary, hit.summaryMatchRanges, highlightColor)
        }
    Spacer(Modifier.height(4.dp))
    Text(
        text = summaryText,
        style = MaterialTheme.typography.bodyMedium,
        color =
            if (missing) {
                MaterialTheme.colorScheme.onSurfaceVariant
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
    )
}

@Composable
private fun SearchMessage(
    title: String,
    supporting: String?,
    actionLabel: String? = null,
    onAction: (() -> Unit)? = null,
) {
    val dims = LocalSujianDimensions.current
    Column(
        modifier =
            Modifier
                .fillMaxSize()
                .padding(horizontal = dims.space32),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(
            imageVector = SujianIcons.Search,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.outline,
        )
        Spacer(Modifier.height(dims.space12))
        Text(
            text = title,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onSurface,
        )
        if (!supporting.isNullOrBlank()) {
            Spacer(Modifier.height(dims.space8))
            Text(
                text = supporting,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (actionLabel != null && onAction != null) {
            Spacer(Modifier.height(dims.space8))
            TextButton(onClick = onAction) {
                Text(actionLabel)
            }
        }
    }
}
