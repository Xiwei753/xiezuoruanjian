package com.xiwei.sujian.core.interop.annotations

import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import uniffi.writer_core.AnnotationEditDto
import uniffi.writer_core.AnnotationNamedEntryDto
import uniffi.writer_core.AnnotationObjectLinkDto
import uniffi.writer_core.AnnotationObjectRefDto
import uniffi.writer_core.AnnotationTagBindingDto
import uniffi.writer_core.AnnotationTagDto
import uniffi.writer_core.AnnotationTargetDto
import uniffi.writer_core.AnnotationTextAnchorDto
import uniffi.writer_core.ChapterAnnotationsDto
import uniffi.writer_core.EditorTransactionCauseDto

/** Typed Android boundary for the Core annotation API. */
class AnnotationBridge internal constructor(
    private val holder: WriterAppServiceHolder,
) {
    fun listTags(projectId: String): BridgeResult<List<AnnotationTagDto>> =
        holder.wrapResult { holder.service.listTags(projectId) }

    fun createTag(
        projectId: String,
        name: String,
        color: String? = null,
    ): BridgeResult<AnnotationTagDto> =
        holder.wrapResult { holder.service.createTag(projectId, name, color) }

    fun renameTag(
        projectId: String,
        tagId: String,
        name: String,
    ): BridgeResult<AnnotationTagDto> =
        holder.wrapResult { holder.service.renameTag(projectId, tagId, name) }

    fun deleteTag(
        projectId: String,
        tagId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult { holder.service.deleteTag(projectId, tagId) }

    fun mergeTags(
        projectId: String,
        sourceTagId: String,
        targetTagId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult { holder.service.mergeTags(projectId, sourceTagId, targetTagId) }

    fun setTagBindings(
        projectId: String,
        target: AnnotationObjectRefDto,
        tagIds: List<String>,
    ): BridgeResult<List<AnnotationTagBindingDto>> =
        holder.wrapResult { holder.service.setTagBindings(projectId, target, tagIds) }

    fun listBindingsForTarget(
        projectId: String,
        target: AnnotationObjectRefDto,
    ): BridgeResult<List<AnnotationTagBindingDto>> =
        holder.wrapResult { holder.service.listBindingsForTarget(projectId, target) }

    fun createTextAnchor(
        projectId: String,
        chapterId: String,
        startByte: Int,
        endByte: Int,
        textRevision: Long,
        text: String,
    ): BridgeResult<AnnotationTextAnchorDto> =
        holder.wrapResult {
            holder.service.createTextAnchor(
                projectId,
                chapterId,
                startByte.toUInt(),
                endByte.toUInt(),
                textRevision.toULong(),
                text,
            )
        }

    fun createNamedEntry(
        projectId: String,
        canonicalName: String,
        aliases: List<String> = emptyList(),
        confirmedMisspellings: List<String> = emptyList(),
        autoLinkEnabled: Boolean = true,
    ): BridgeResult<AnnotationNamedEntryDto> =
        holder.wrapResult {
            holder.service.createNamedEntry(
                projectId,
                canonicalName,
                aliases,
                confirmedMisspellings,
                autoLinkEnabled,
            )
        }

    fun updateNamedEntry(
        projectId: String,
        entryId: String,
        canonicalName: String,
        aliases: List<String>,
        confirmedMisspellings: List<String>,
        autoLinkEnabled: Boolean,
    ): BridgeResult<AnnotationNamedEntryDto> =
        holder.wrapResult {
            holder.service.updateNamedEntry(
                projectId,
                entryId,
                canonicalName,
                aliases,
                confirmedMisspellings,
                autoLinkEnabled,
            )
        }

    fun deleteNamedEntry(
        projectId: String,
        entryId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult { holder.service.deleteNamedEntry(projectId, entryId) }

    fun listNamedEntries(projectId: String): BridgeResult<List<AnnotationNamedEntryDto>> =
        holder.wrapResult { holder.service.listNamedEntries(projectId) }

    fun createLink(
        source: AnnotationObjectRefDto,
        target: AnnotationTargetDto,
    ): BridgeResult<AnnotationObjectLinkDto> =
        holder.wrapResult { holder.service.createLink(source, target) }

    fun updateLink(
        projectId: String,
        linkId: String,
        source: AnnotationObjectRefDto,
        target: AnnotationTargetDto,
    ): BridgeResult<AnnotationObjectLinkDto> =
        holder.wrapResult { holder.service.updateLink(projectId, linkId, source, target) }

    fun deleteLink(
        projectId: String,
        linkId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult { holder.service.deleteLink(projectId, linkId) }

    fun resolveLink(target: AnnotationTargetDto): BridgeResult<uniffi.writer_core.AnnotationLinkResolutionDto> =
        holder.wrapResult { holder.service.resolveLink(target) }

    fun findBacklinks(target: AnnotationTargetDto): BridgeResult<List<AnnotationObjectLinkDto>> =
        holder.wrapResult { holder.service.findBacklinks(target) }

    fun listLinksForSource(source: AnnotationObjectRefDto): BridgeResult<List<AnnotationObjectLinkDto>> =
        holder.wrapResult { holder.service.listLinksForSource(source) }

    fun getChapterAnnotations(
        projectId: String,
        chapterId: String,
        textRevision: Long,
        text: String,
    ): BridgeResult<ChapterAnnotationsDto> =
        holder.wrapResult {
            holder.service.getChapterAnnotations(projectId, chapterId, textRevision.toULong(), text)
        }

    fun applyAnnotationEdits(
        projectId: String,
        chapterId: String,
        baseRevision: Long,
        textRevision: Long,
        cause: EditorTransactionCauseDto,
        edits: List<AnnotationEditDto>,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.applyAnnotationEdits(
                projectId,
                chapterId,
                baseRevision.toULong(),
                textRevision.toULong(),
                cause,
                edits,
            )
        }

    fun findAutoNameLinks(
        projectId: String,
        chapterId: String,
        textRevision: Long,
        text: String,
    ): BridgeResult<List<uniffi.writer_core.AnnotationAutoMatchDto>> =
        holder.wrapResult {
            holder.service.findAutoNameLinks(projectId, chapterId, textRevision.toULong(), text)
        }
}
