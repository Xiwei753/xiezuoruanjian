import { resourceManager } from '@kit.LocalizationKit'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import {
  CorrectionChapterSnapshotDto,
  CorrectionModelPathStatusDto,
  CorrectionResourceStatusDto
} from './CorrectionDtos'

export type CorrectionIgnoreScope = 'once' | 'chapter' | 'permanent' | 'rule'

/** Harmony transport contract for the Core-owned correction operations. */
export interface ICorrectionBridge {
  setModelPath(path: string): Promise<ResultEnvelope<boolean>>
  getResourceStatus(): Promise<ResultEnvelope<CorrectionResourceStatusDto>>
  getModelPathStatus(path: string): Promise<ResultEnvelope<CorrectionModelPathStatusDto>>
  /**
   * Issue #985: stream one bundled ONNX model resource out of the HAP rawfile via the
   * native OH_ResourceManager APIs and resolve the verified sandbox path.
   * Rejects with an Error when the rawfile or the copy/digest check fails.
   */
  prepareBundledCorrectionModel(manager: resourceManager.ResourceManager, rawFileName: string,
    destinationPath: string, expectedSize: number, expectedSha256: string): Promise<string>
  checkSegment(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    startByte: number, endByte: number): Promise<ResultEnvelope<CorrectionChapterSnapshotDto>>
  checkEdits(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    baseRevision: number, text: string,
    editsJson: string): Promise<ResultEnvelope<CorrectionChapterSnapshotDto>>
  /** Lightweight edit phase: update Core's segment index without running the model. */
  applyEdits(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    baseRevision: number, editsJson: string): Promise<ResultEnvelope<boolean>>
  /** Model phase: scan only the dirty windows recorded by applyEdits. */
  scanPending(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    text: string): Promise<ResultEnvelope<CorrectionChapterSnapshotDto>>
  checkChapter(sessionId: number, projectId: string, chapterId: string,
    textRevision: number): Promise<ResultEnvelope<CorrectionChapterSnapshotDto>>
  getChapterSnapshot(sessionId: number, projectId: string, chapterId: string,
    textRevision: number): Promise<ResultEnvelope<CorrectionChapterSnapshotDto>>
  ignore(projectId: string, chapterId: string, ruleId: string, original: string,
    scope: CorrectionIgnoreScope, textRevision: number, startByte: number,
    endByte: number): Promise<ResultEnvelope<boolean>>
  addDictionaryEntry(projectId: string, entry: string): Promise<ResultEnvelope<boolean>>
  apply(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    startByte: number, endByte: number, original: string,
    replacement: string): Promise<ResultEnvelope<EditorEditResult>>
}
