import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { EditorEditResult } from '../../../corebridge/dto/EditorDtos'
import {
  CorrectionModelPathStatusDto,
  CorrectionResourceStatusDto,
  CorrectionSuggestionDto
} from './CorrectionDtos'

export type CorrectionIgnoreScope = 'once' | 'chapter' | 'permanent' | 'rule'

/** Harmony transport contract for the Core-owned correction operations. */
export interface ICorrectionBridge {
  setModelPath(path: string): Promise<ResultEnvelope<boolean>>
  getResourceStatus(): Promise<ResultEnvelope<CorrectionResourceStatusDto>>
  getModelPathStatus(path: string): Promise<ResultEnvelope<CorrectionModelPathStatusDto>>
  checkSegment(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    startByte: number, endByte: number): Promise<ResultEnvelope<CorrectionSuggestionDto[]>>
  checkChapter(sessionId: number, projectId: string, chapterId: string,
    textRevision: number): Promise<ResultEnvelope<CorrectionSuggestionDto[]>>
  ignore(projectId: string, chapterId: string, ruleId: string, original: string,
    scope: CorrectionIgnoreScope, textRevision: number, startByte: number,
    endByte: number): Promise<ResultEnvelope<boolean>>
  addDictionaryEntry(projectId: string, entry: string): Promise<ResultEnvelope<boolean>>
  apply(sessionId: number, projectId: string, chapterId: string, textRevision: number,
    startByte: number, endByte: number, original: string,
    replacement: string): Promise<ResultEnvelope<EditorEditResult>>
}
