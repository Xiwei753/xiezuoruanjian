package com.xiwei.sujian.feature.editor.presentation

import com.xiwei.sujian.core.interop.annotations.AnnotationBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.editor.input.TextOffsetUtils
import com.xiwei.sujian.feature.search.data.SearchRepository
import com.xiwei.sujian.feature.search.data.model.SearchContext
import com.xiwei.sujian.feature.search.data.model.SearchHit
import com.xiwei.sujian.feature.search.data.model.SearchScopes
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import uniffi.writer_core.AnnotationEditDto
import uniffi.writer_core.AnnotationObjectKindDto
import uniffi.writer_core.AnnotationObjectRefDto
import uniffi.writer_core.AnnotationLinkResolutionStateDto
import uniffi.writer_core.AnnotationTargetDto
import uniffi.writer_core.EditorTransactionCauseDto

enum class AnnotationObjectKind {
    Project,
    Volume,
    Chapter,
    TextAnchor,
    StarMap,
    StarMapNode,
    StarMapEmbed,
    NamedEntry,
}

data class AnnotationObjectRef(
    val kind: AnnotationObjectKind,
    val projectId: String,
    val objectId: String,
)

data class AnnotationUiTag(
    val id: String,
    val name: String,
    val color: String?,
)

data class AnnotationUiLink(
    val id: String,
    val label: String,
    val target: AnnotationObjectRef?,
    val externalUrl: String?,
)

data class AnnotationUiRange(
    val start: Int,
    val end: Int,
)

enum class AnnotationLinkOrigin {
    Manual,
    Automatic,
}

data class AnnotationUiLinkTarget(
    val ref: AnnotationObjectRef? = null,
    val externalUrl: String? = null,
    val label: String,
)

data class AnnotationUiLinkedRange(
    val range: AnnotationUiRange,
    val targets: List<AnnotationUiLinkTarget>,
    val origin: AnnotationLinkOrigin,
)

data class AnnotationResolvedLinkTarget(
    val target: AnnotationUiLinkTarget,
    val state: AnnotationLinkResolutionStateDto,
)

data class AnnotationLinkOpenResult(
    val origin: AnnotationLinkOrigin,
    val targets: List<AnnotationResolvedLinkTarget>,
)

data class AnnotationSearchTarget(
    val title: String,
    val subtitle: String,
    val ref: AnnotationObjectRef,
)

data class AnnotationCoordinatorState(
    val isSheetOpen: Boolean = false,
    val isLoading: Boolean = false,
    val isSaving: Boolean = false,
    val projectId: String? = null,
    val targetLabel: String = "",
    val selectedText: String = "",
    val nameDraft: String = "",
    val tags: List<AnnotationUiTag> = emptyList(),
    val selectedTagIds: Set<String> = emptySet(),
    val outgoingLinks: List<AnnotationUiLink> = emptyList(),
    val backlinks: List<AnnotationUiLink> = emptyList(),
    val namedEntries: List<String> = emptyList(),
    val tagRanges: List<AnnotationUiRange> = emptyList(),
    val linkRanges: List<AnnotationUiRange> = emptyList(),
    val linkedRanges: List<AnnotationUiLinkedRange> = emptyList(),
    val linkSearchQuery: String = "",
    val linkSearchResults: List<AnnotationSearchTarget> = emptyList(),
    val error: String? = null,
)

/** Maps Core annotation snapshots and committed editor transactions for the active chapter. */
class AnnotationCoordinator(
    private val bridge: AnnotationBridge,
    private val searchRepository: SearchRepository,
    private val scope: CoroutineScope,
) {
    private val _state = MutableStateFlow(AnnotationCoordinatorState())
    val state: StateFlow<AnnotationCoordinatorState> = _state.asStateFlow()
    private val mutationMutex = Mutex()
    private var targetContext: TargetContext? = null
    private var requestGeneration = 0L

    fun loadChapter(
        projectId: String,
        chapterId: String,
        revision: Long,
        text: String,
    ) {
        targetContext =
            TargetContext(
                projectId = projectId,
                chapterId = chapterId,
                revision = revision,
                text = text,
                objectRef = AnnotationObjectRef(AnnotationObjectKind.Chapter, projectId, chapterId),
                targetLabel = chapterId,
            )
        reload()
    }

    fun openChapter(
        projectId: String,
        chapterId: String,
        revision: Long,
        text: String,
        title: String,
    ) {
        loadChapter(projectId, chapterId, revision, text)
        _state.update { it.copy(isSheetOpen = true, targetLabel = title) }
    }

    fun openSelection(
        projectId: String,
        chapterId: String,
        revision: Long,
        text: String,
        startByte: Int,
        endByte: Int,
        chapterTitle: String,
    ) {
        val start = minOf(startByte, endByte).coerceAtLeast(0)
        val end = maxOf(startByte, endByte).coerceAtLeast(start)
        val selection = TextOffsetUtils.utf16TextRangeForUtf8(text, start, end)
        targetContext =
            TargetContext(
                projectId = projectId,
                chapterId = chapterId,
                revision = revision,
                text = text,
                selectedByteRange = start to end,
                targetLabel = chapterTitle,
            )
        _state.update {
            it.copy(
                isSheetOpen = true,
                projectId = projectId,
                targetLabel = chapterTitle,
                selectedText = text.substring(selection.start, selection.end),
                nameDraft = text.substring(selection.start, selection.end),
                error = null,
            )
        }
        reload()
    }

    fun openObject(
        ref: AnnotationObjectRef,
        label: String,
    ) {
        targetContext =
            TargetContext(
                projectId = ref.projectId,
                objectRef = ref,
                targetLabel = label,
            )
        _state.update {
            it.copy(
                isSheetOpen = true,
                projectId = ref.projectId,
                targetLabel = label,
                selectedText = "",
                nameDraft = "",
                linkSearchQuery = "",
                linkSearchResults = emptyList(),
                error = null,
            )
        }
        reload()
    }

    fun dismissSheet() {
        _state.update { it.copy(isSheetOpen = false, error = null) }
    }

    /** Resolve the manual or automatic targets at a text offset; manual links take precedence. */
    fun resolveLinkedText(
        utf16Offset: Int,
        onResolved: (AnnotationLinkOpenResult) -> Unit,
    ) {
        val context = targetContext ?: return
        val linkedRanges = _state.value.linkedRanges
        val hitRanges = linkedRanges.filter { linkedRange ->
            utf16Offset in linkedRange.range.start until linkedRange.range.end ||
                (utf16Offset > 0 && utf16Offset - 1 in linkedRange.range.start until linkedRange.range.end)
        }
        val manual = hitRanges.filter { it.origin == AnnotationLinkOrigin.Manual }
        val selectedRanges = manual.ifEmpty { hitRanges.filter { it.origin == AnnotationLinkOrigin.Automatic } }
        val origin = if (manual.isNotEmpty()) AnnotationLinkOrigin.Manual else AnnotationLinkOrigin.Automatic
        val targets = selectedRanges.flatMap { it.targets }.distinctBy { target ->
            target.ref?.let { "${it.kind}:${it.projectId}:${it.objectId}" } ?: "url:${target.externalUrl}"
        }
        if (targets.isEmpty()) return

        scope.launch {
            val resolved =
                runCatching {
                    withContext(Dispatchers.IO) {
                        targets.map { target ->
                            val dto =
                                AnnotationTargetDto(
                                    objectRef = target.ref?.toDto(),
                                    externalUrl = target.externalUrl,
                                )
                            val resolution = requireData(bridge.resolveLink(dto))
                            AnnotationResolvedLinkTarget(target, resolution.state)
                        }
                    }
                }.getOrElse { error ->
                    _state.update { it.copy(error = error.message ?: "Could not resolve link target") }
                    return@launch
                }
            val latest = targetContext
            if (latest?.projectId == context.projectId &&
                latest.chapterId == context.chapterId &&
                latest.revision == context.revision &&
                latest.text == context.text
            ) {
                onResolved(AnnotationLinkOpenResult(origin, resolved))
            }
        }
    }

    fun onNameDraftChanged(value: String) {
        _state.update { it.copy(nameDraft = value) }
    }

    fun onLinkSearchChanged(value: String) {
        _state.update { it.copy(linkSearchQuery = value, linkSearchResults = emptyList()) }
        val context = targetContext ?: return
        val query = value.trim()
        if (query.isEmpty()) return
        scope.launch {
            val results =
                runCatching {
                    withContext(Dispatchers.IO) {
                        searchRepository.searchPage(
                            query = query,
                            scope = SearchScopes.All,
                            limit = 25,
                            cursor = null,
                            searchContext =
                                SearchContext(
                                    originScreen = "writing",
                                    currentProjectId = context.projectId,
                                    currentChapterId = context.chapterId,
                                ),
                        ).items.mapNotNull(::toSearchTarget)
                    }
                }.getOrElse { error ->
                    _state.update { it.copy(error = error.message ?: "Search failed") }
                    emptyList()
                }
            if (targetContext === context) {
                _state.update { it.copy(linkSearchResults = results) }
            }
        }
    }

    fun toggleTag(tagId: String, enabled: Boolean) {
        scope.launch {
            mutationMutex.withLock {
                mutate {
                    val ref = ensureObjectRef() ?: error("Select a chapter range or object first")
                    val selected = _state.value.selectedTagIds.toMutableSet()
                    if (enabled) selected += tagId else selected -= tagId
                    requireData(bridge.setTagBindings(ref.projectId, ref.toDto(), selected.toList()))
                    Unit
                }
            }
        }
    }

    fun createTag(name: String) {
        val cleanedName = name.trim()
        if (cleanedName.isEmpty()) return
        scope.launch {
            mutationMutex.withLock {
                mutate {
                    val context = targetContext ?: error("No annotation target is open")
                    val created = requireData(bridge.createTag(context.projectId, cleanedName))
                    val ref = ensureObjectRef()
                    if (ref != null) {
                        val tagIds = _state.value.selectedTagIds + created.tagId
                        requireData(bridge.setTagBindings(context.projectId, ref.toDto(), tagIds.toList()))
                    }
                    Unit
                }
            }
        }
    }

    fun addExternalLink(url: String) {
        val cleanedUrl = url.trim()
        if (cleanedUrl.isEmpty()) return
        scope.launch {
            mutationMutex.withLock {
                mutate {
                    val ref = ensureObjectRef() ?: error("Select a chapter range or object first")
                    requireData(
                        bridge.createLink(
                            ref.toDto(),
                            AnnotationTargetDto(objectRef = null, externalUrl = cleanedUrl),
                        ),
                    )
                    Unit
                }
            }
        }
    }

    fun addObjectLink(target: AnnotationObjectRef) {
        scope.launch {
            mutationMutex.withLock {
                mutate {
                    val source = ensureObjectRef() ?: error("Select a chapter range or object first")
                    requireData(
                        bridge.createLink(
                            source.toDto(),
                            AnnotationTargetDto(objectRef = target.toDto(), externalUrl = null),
                        ),
                    )
                    Unit
                }
            }
        }
    }

    fun deleteLink(linkId: String) {
        scope.launch {
            mutationMutex.withLock {
                mutate {
                    val context = targetContext ?: error("No annotation target is open")
                    requireData(bridge.deleteLink(context.projectId, linkId))
                    Unit
                }
            }
        }
    }

    fun registerName() {
        val canonicalName = _state.value.nameDraft.trim()
        if (canonicalName.isEmpty()) return
        scope.launch {
            mutationMutex.withLock {
                mutate {
                    val context = targetContext ?: error("No annotation target is open")
                    requireData(bridge.createNamedEntry(context.projectId, canonicalName))
                    Unit
                }
            }
        }
    }

    /** Called for every committed Core edit; range remapping uses its actual offset map. */
    suspend fun onEditFact(
        projectId: String,
        chapterId: String,
        baseRevision: Long,
        newRevision: Long,
        cause: EditorTransactionCauseDto,
        oldText: String,
        newText: String,
        offsetMap: com.xiwei.sujian.feature.editor.projection.OffsetMap?,
    ) {
        if (offsetMap == null || oldText == newText) return
        val edits = annotationEditRanges(oldText, newText, offsetMap)
        if (edits.isEmpty()) return
        var context = targetContext?.takeIf { it.projectId == projectId && it.chapterId == chapterId } ?: return
        val baselineId =
            context.baselineId ?: try {
                withContext(Dispatchers.IO) {
                    requireData(
                        bridge.getChapterAnnotations(
                            projectId,
                            chapterId,
                            baseRevision,
                            oldText,
                        ),
                    ).baselineId
                }
            } catch (error: Exception) {
                _state.update { it.copy(error = error.message ?: "Could not establish annotation baseline") }
                return
            }
        val latestContext = targetContext?.takeIf { it.projectId == projectId && it.chapterId == chapterId } ?: return
        if (latestContext.revision != context.revision || latestContext.text != context.text) return
        context = latestContext.copy(baselineId = latestContext.baselineId ?: baselineId)
        targetContext = context
        val activeBaselineId = context.baselineId ?: return
        try {
            withContext(Dispatchers.IO) {
                requireData(
                    bridge.applyAnnotationEdits(
                        projectId,
                        chapterId,
                        activeBaselineId,
                        baseRevision,
                        newRevision,
                        cause,
                        edits,
                        newText,
                    ),
                )
            }
        } catch (error: Exception) {
            _state.update { it.copy(error = error.message ?: "Could not update annotation ranges") }
            return
        }
        val updatedContext = targetContext
        if (updatedContext?.projectId == projectId && updatedContext.chapterId == chapterId) {
            targetContext = updatedContext.copy(revision = newRevision, text = newText)
            reload()
        }
    }

    private suspend fun mutate(block: suspend () -> Unit) {
        _state.update { it.copy(isSaving = true, error = null) }
        try {
            withContext(Dispatchers.IO) { block() }
            refresh()
        } catch (error: Exception) {
            _state.update { it.copy(isSaving = false, error = error.message ?: "Annotation update failed") }
        }
    }

    private fun reload() {
        val context = targetContext ?: return
        val generation = ++requestGeneration
        _state.update { it.copy(projectId = context.projectId, isLoading = true, error = null) }
        scope.launch {
            try {
                val data = withContext(Dispatchers.IO) { loadSnapshot(context) }
                if (generation == requestGeneration) {
                    _state.value = data.copy(isSheetOpen = _state.value.isSheetOpen)
                }
            } catch (error: Exception) {
                if (generation == requestGeneration) {
                    _state.update {
                        it.copy(
                            isLoading = false,
                            isSaving = false,
                            error = error.message ?: "Could not load annotations",
                        )
                    }
                }
            }
        }
    }

    private suspend fun refresh() {
        val context = targetContext ?: return
        val data = withContext(Dispatchers.IO) { loadSnapshot(context) }
        _state.value = data.copy(isSheetOpen = _state.value.isSheetOpen)
    }

    private suspend fun loadSnapshot(context: TargetContext): AnnotationCoordinatorState {
        val tags = requireData(bridge.listTags(context.projectId)).filter { it.deletedAtMs == null }
        val chapterSnapshot =
            if (context.chapterId != null && context.text != null) {
                requireData(
                    bridge.getChapterAnnotations(
                        context.projectId,
                        context.chapterId,
                        context.revision,
                        context.text,
                    ),
                )
            } else {
                null
            }
        if (chapterSnapshot != null && targetContext === context) {
            targetContext = context.copy(baselineId = chapterSnapshot.baselineId)
        }
        val ref = context.objectRef ?: chapterSnapshot?.anchors?.firstOrNull { anchor ->
            context.selectedByteRange?.let { (start, end) ->
                anchor.startByte.toInt() == start && anchor.endByte.toInt() == end && anchor.deletedAtMs == null
            } ?: false
        }?.let { AnnotationObjectRef(AnnotationObjectKind.TextAnchor, context.projectId, it.anchorId) }
        val bindings =
            if (ref != null) requireData(bridge.listBindingsForTarget(context.projectId, ref.toDto())) else emptyList()
        val outgoing =
            if (ref != null) requireData(bridge.listLinksForSource(ref.toDto())) else emptyList()
        val backlinks =
            if (ref != null) {
                requireData(
                    bridge.findBacklinks(
                        AnnotationTargetDto(objectRef = ref.toDto(), externalUrl = null),
                    ),
                )
            } else {
                emptyList()
            }
        val entries = requireData(bridge.listNamedEntries(context.projectId)).filter { it.deletedAtMs == null }
        val text = context.text.orEmpty()
        val anchorById = chapterSnapshot?.anchors.orEmpty().associateBy { it.anchorId }
        val activeBindings = chapterSnapshot?.bindings.orEmpty().filter { it.deletedAtMs == null }
        val activeTagIds = tags.mapTo(mutableSetOf()) { it.tagId }
        val tagRanges =
            chapterSnapshot?.anchors.orEmpty().mapNotNull { anchor ->
                val hasTag =
                    activeBindings.any { binding ->
                        binding.target.objectId == anchor.anchorId && binding.tagId in activeTagIds
                    }
                if (!hasTag || anchor.deletedAtMs != null) null else anchor.toUiRange(text)
            }
        val namedEntriesById = entries.associateBy { it.entryId }
        val linkedRanges = buildList {
            chapterSnapshot?.links.orEmpty()
                .filter { it.deletedAtMs == null && it.source.kind == AnnotationObjectKindDto.TEXT_ANCHOR }
                .groupBy { it.source.objectId }
                .forEach { (anchorId, links) ->
                    val anchor = anchorById[anchorId]?.takeIf { it.deletedAtMs == null } ?: return@forEach
                    val range = anchor.toUiRange(text) ?: return@forEach
                    val targets =
                        links.mapNotNull { link ->
                            val objectRef = link.target.objectRef?.toUi()
                            val externalUrl = link.target.externalUrl
                            if (objectRef == null && externalUrl.isNullOrBlank()) {
                                null
                            } else {
                                AnnotationUiLinkTarget(
                                    ref = objectRef,
                                    externalUrl = externalUrl,
                                    label =
                                        externalUrl
                                            ?: objectRef?.let { ref ->
                                                namedEntriesById[ref.objectId]?.canonicalName ?: ref.objectId
                                            }
                                            ?: "Link",
                                )
                            }
                        }
                    if (targets.isNotEmpty()) add(AnnotationUiLinkedRange(range, targets, AnnotationLinkOrigin.Manual))
                }
            chapterSnapshot?.autoMatches.orEmpty().forEach { match ->
                val range = match.toUiRange(text) ?: return@forEach
                val targets =
                    match.candidates.map { candidate ->
                        val ref = candidate.toUi()
                        AnnotationUiLinkTarget(
                            ref = ref,
                            label = namedEntriesById[ref.objectId]?.canonicalName ?: ref.objectId,
                        )
                    }
                if (targets.isNotEmpty()) add(AnnotationUiLinkedRange(range, targets, AnnotationLinkOrigin.Automatic))
            }
        }
        val linkRanges = linkedRanges.map { it.range }
        val selectedIds = bindings.filter { it.deletedAtMs == null }.map { it.tagId }.toSet()
        return AnnotationCoordinatorState(
            isSheetOpen = _state.value.isSheetOpen,
            projectId = context.projectId,
            targetLabel = context.targetLabel,
            selectedText = _state.value.selectedText,
            nameDraft = _state.value.nameDraft,
            tags = tags.map { AnnotationUiTag(it.tagId, it.name, it.color) },
            selectedTagIds = selectedIds,
            outgoingLinks = outgoing.filter { it.deletedAtMs == null }.map { it.toUi() },
            backlinks = backlinks.filter { it.deletedAtMs == null }.map { it.toUi() },
            namedEntries = entries.map { it.canonicalName },
            tagRanges = tagRanges,
            linkRanges = linkRanges,
            linkedRanges = linkedRanges,
            linkSearchQuery = _state.value.linkSearchQuery,
            linkSearchResults = _state.value.linkSearchResults,
            isLoading = false,
            isSaving = false,
        )
    }

    private suspend fun ensureObjectRef(): AnnotationObjectRefDto? {
        val context = targetContext ?: return null
        context.objectRef?.let { return it.toDto() }
        val range = context.selectedByteRange ?: return null
        val chapter = context.chapterId ?: return null
        val text = context.text ?: return null
        val matchingAnchor =
            requireData(
                bridge.getChapterAnnotations(
                    context.projectId,
                    chapter,
                    context.revision,
                    text,
                ),
            ).anchors.firstOrNull { anchor ->
                anchor.startByte.toInt() == range.first &&
                    anchor.endByte.toInt() == range.second &&
                    anchor.deletedAtMs == null
            }
        if (matchingAnchor != null) {
            val ref = AnnotationObjectRef(AnnotationObjectKind.TextAnchor, context.projectId, matchingAnchor.anchorId)
            targetContext = context.copy(objectRef = ref)
            return ref.toDto()
        }
        val anchor =
            requireData(
                bridge.createTextAnchor(
                    context.projectId,
                    chapter,
                    range.first,
                    range.second,
                    context.revision,
                    text,
                ),
            )
        val ref = AnnotationObjectRef(AnnotationObjectKind.TextAnchor, context.projectId, anchor.anchorId)
        targetContext = context.copy(objectRef = ref)
        return ref.toDto()
    }

    private fun toSearchTarget(hit: SearchHit): AnnotationSearchTarget? {
        val target = hit.target
        val projectId = target.projectId ?: return null
        val ref =
            when {
                target.nodeId != null -> AnnotationObjectRef(AnnotationObjectKind.StarMapNode, projectId, target.nodeId)
                target.embedId != null ->
                    AnnotationObjectRef(AnnotationObjectKind.StarMapEmbed, projectId, target.embedId)
                target.chapterId != null ->
                    AnnotationObjectRef(AnnotationObjectKind.Chapter, projectId, target.chapterId)
                target.volumeId != null -> AnnotationObjectRef(AnnotationObjectKind.Volume, projectId, target.volumeId)
                target.starmapId != null ->
                    AnnotationObjectRef(AnnotationObjectKind.StarMap, projectId, target.starmapId)
                else -> AnnotationObjectRef(AnnotationObjectKind.Project, projectId, projectId)
            }
        if (targetContext?.objectRef == ref) return null
        return AnnotationSearchTarget(hit.title, hit.path, ref)
    }

    private data class TargetContext(
        val projectId: String,
        val chapterId: String? = null,
        val revision: Long = 0L,
        val text: String? = null,
        val selectedByteRange: Pair<Int, Int>? = null,
        val objectRef: AnnotationObjectRef? = null,
        val targetLabel: String,
        val baselineId: String? = null,
    )
}

private fun AnnotationObjectRef.toDto(): AnnotationObjectRefDto =
    AnnotationObjectRefDto(
        kind =
            when (kind) {
                AnnotationObjectKind.Project -> AnnotationObjectKindDto.PROJECT
                AnnotationObjectKind.Volume -> AnnotationObjectKindDto.VOLUME
                AnnotationObjectKind.Chapter -> AnnotationObjectKindDto.CHAPTER
                AnnotationObjectKind.TextAnchor -> AnnotationObjectKindDto.TEXT_ANCHOR
                AnnotationObjectKind.StarMap -> AnnotationObjectKindDto.STAR_MAP
                AnnotationObjectKind.StarMapNode -> AnnotationObjectKindDto.STAR_MAP_NODE
                AnnotationObjectKind.StarMapEmbed -> AnnotationObjectKindDto.STAR_MAP_EMBED
                AnnotationObjectKind.NamedEntry -> AnnotationObjectKindDto.NAMED_ENTRY
            },
        projectId = projectId,
        objectId = objectId,
    )

private fun uniffi.writer_core.AnnotationObjectRefDto.toUi(): AnnotationObjectRef =
    AnnotationObjectRef(
        kind =
            when (kind) {
                AnnotationObjectKindDto.PROJECT -> AnnotationObjectKind.Project
                AnnotationObjectKindDto.VOLUME -> AnnotationObjectKind.Volume
                AnnotationObjectKindDto.CHAPTER -> AnnotationObjectKind.Chapter
                AnnotationObjectKindDto.TEXT_ANCHOR -> AnnotationObjectKind.TextAnchor
                AnnotationObjectKindDto.STAR_MAP -> AnnotationObjectKind.StarMap
                AnnotationObjectKindDto.STAR_MAP_NODE -> AnnotationObjectKind.StarMapNode
                AnnotationObjectKindDto.STAR_MAP_EMBED -> AnnotationObjectKind.StarMapEmbed
                AnnotationObjectKindDto.NAMED_ENTRY -> AnnotationObjectKind.NamedEntry
            },
        projectId = projectId,
        objectId = objectId,
    )

private fun uniffi.writer_core.AnnotationObjectLinkDto.toUi(): AnnotationUiLink =
    AnnotationUiLink(
        id = linkId,
        label = target.externalUrl ?: target.objectRef?.objectId.orEmpty(),
        target = target.objectRef?.toUi(),
        externalUrl = target.externalUrl,
    )

private fun uniffi.writer_core.AnnotationTextAnchorDto.toUiRange(text: String): AnnotationUiRange? {
    val range = TextOffsetUtils.utf16TextRangeForUtf8(text, startByte.toInt(), endByte.toInt())
    return if (range.collapsed || range.end > text.length) null else AnnotationUiRange(range.start, range.end)
}

private fun uniffi.writer_core.AnnotationAutoMatchDto.toUiRange(text: String): AnnotationUiRange? {
    val range = TextOffsetUtils.utf16TextRangeForUtf8(text, startByte.toInt(), endByte.toInt())
    return if (range.collapsed || range.end > text.length) null else AnnotationUiRange(range.start, range.end)
}

private fun <T> requireData(result: BridgeResult<T>): T =
    when (result) {
        is BridgeResult.Success -> result.data
        is BridgeResult.Error -> error(result.envelope.rawError ?: result.message)
        BridgeResult.NotLoaded -> error("Annotation service is unavailable")
    }

private fun annotationEditRanges(
    oldText: String,
    newText: String,
    offsetMap: com.xiwei.sujian.feature.editor.projection.OffsetMap,
): List<AnnotationEditDto> {
    val entries = offsetMap.entries.sortedBy { it.oldByteOffset }
    val result = mutableListOf<AnnotationEditDto>()
    var oldCursor = 0
    var newCursor = 0
    entries.forEach { entry ->
        val oldStart = entry.oldByteOffset.coerceAtLeast(oldCursor)
        val newStart = entry.newByteOffset.coerceAtLeast(newCursor)
        if (oldStart != oldCursor || newStart != newCursor) {
            result += AnnotationEditDto(
                oldStartByte = oldCursor.toUInt(),
                oldEndByte = oldStart.toUInt(),
                newStartByte = newCursor.toUInt(),
                newEndByte = newStart.toUInt(),
            )
        }
        oldCursor = oldStart + entry.length
        newCursor = newStart + entry.length
    }
    val oldLength = oldText.toByteArray(Charsets.UTF_8).size
    val newLength = newText.toByteArray(Charsets.UTF_8).size
    if (oldCursor != oldLength || newCursor != newLength) {
        result += AnnotationEditDto(
            oldStartByte = oldCursor.toUInt(),
            oldEndByte = oldLength.toUInt(),
            newStartByte = newCursor.toUInt(),
            newEndByte = newLength.toUInt(),
        )
    }
    return result
}
