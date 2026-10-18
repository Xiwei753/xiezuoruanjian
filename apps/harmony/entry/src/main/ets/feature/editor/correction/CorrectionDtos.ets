import { DecodeResult } from '../../../corebridge/codec/CoreWireDecoders'

/** Canonical Core correction suggestion. Offsets are UTF-8 bytes until the UI boundary maps them. */
export interface CorrectionSuggestionDto {
  startByte: number
  endByte: number
  original: string
  replacement: string
  source: string
  ruleId: string
  severity: string
  textRevision: number
}

/** One chapter byte range Core verified against the current text. */
export interface CorrectionCheckedRangeDto {
  startByte: number
  endByte: number
}

/**
 * Core merged chapter snapshot (issue #954). Suggestions are repositioned to the
 * requested revision; unchanged segments reuse cached results without rerunning
 * the model. checkedRanges lists ranges already verified against current text.
 */
export interface CorrectionChapterSnapshotDto {
  textRevision: number
  suggestions: CorrectionSuggestionDto[]
  checkedRanges: CorrectionCheckedRangeDto[]
}

export interface CorrectionResourceStatusDto {
  modelConfigured: boolean
  modelFileAvailable: boolean
  bundledLexiconAvailable: boolean
  modelPath: string | null
  installedDictionaryFiles: string[]
  missingDictionaryFiles: string[]
}

/** Core lifecycle view for one path; deletion is allowed only when Core marks it eligible. */
export interface CorrectionModelPathStatusDto {
  configured: boolean
  inUse: boolean
  deletionEligible: boolean
}

/** A current-revision suggestion projected to ArkTS UTF-16 coordinates for presentation. */
export interface MappedCorrectionSuggestion extends CorrectionSuggestionDto {
  startUtf16: number
  endUtf16: number
}

function isRecord(value: Object): boolean {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function decodeSuggestion(value: Object, index: number): DecodeResult<CorrectionSuggestionDto> {
  if (!isRecord(value)) {
    return { ok: false, error: `suggestions[${index}]: expected object` }
  }
  const record = value as Record<string, Object>
  const startByte = record['startByte']
  const endByte = record['endByte']
  const original = record['original']
  const replacement = record['replacement']
  const source = record['source']
  const ruleId = record['ruleId']
  const severity = record['severity']
  const textRevision = record['textRevision']
  if (typeof startByte !== 'number' || typeof endByte !== 'number' ||
      typeof original !== 'string' || typeof replacement !== 'string' ||
      typeof source !== 'string' || typeof ruleId !== 'string' ||
      typeof severity !== 'string' || typeof textRevision !== 'number') {
    return { ok: false, error: `suggestions[${index}]: invalid CorrectionSuggestionDto fields` }
  }
  return {
    ok: true,
    value: {
      startByte: startByte,
      endByte: endByte,
      original: original,
      replacement: replacement,
      source: source,
      ruleId: ruleId,
      severity: severity,
      textRevision: textRevision
    }
  }
}

function decodeCheckedRange(value: Object, index: number): DecodeResult<CorrectionCheckedRangeDto> {
  if (!isRecord(value)) {
    return { ok: false, error: `checkedRanges[${index}]: expected object` }
  }
  const record = value as Record<string, Object>
  const startByte = record['startByte']
  const endByte = record['endByte']
  if (typeof startByte !== 'number' || typeof endByte !== 'number') {
    return { ok: false, error: `checkedRanges[${index}]: invalid CorrectionCheckedRangeDto fields` }
  }
  return { ok: true, value: { startByte: startByte, endByte: endByte } }
}

/** Strict decoder for the Core merged chapter snapshot returned by every correction check. */
export function decodeCorrectionChapterSnapshot(data: Object): DecodeResult<CorrectionChapterSnapshotDto> {
  if (!isRecord(data)) {
    return { ok: false, error: 'data: expected CorrectionChapterSnapshotDto' }
  }
  const record = data as Record<string, Object>
  const textRevision = record['textRevision']
  const suggestionsValue = record['suggestions']
  const checkedRangesValue = record['checkedRanges']
  if (typeof textRevision !== 'number' || !Array.isArray(suggestionsValue) ||
      !Array.isArray(checkedRangesValue)) {
    return { ok: false, error: 'data: invalid CorrectionChapterSnapshotDto fields' }
  }
  const suggestions: CorrectionSuggestionDto[] = []
  for (let i = 0; i < suggestionsValue.length; i++) {
    const decoded = decodeSuggestion(suggestionsValue[i], i)
    if (!decoded.ok || decoded.value === undefined) {
      return { ok: false, error: decoded.error ?? `suggestions[${i}]: decode failed` }
    }
    suggestions.push(decoded.value)
  }
  const checkedRanges: CorrectionCheckedRangeDto[] = []
  for (let i = 0; i < checkedRangesValue.length; i++) {
    const decoded = decodeCheckedRange(checkedRangesValue[i], i)
    if (!decoded.ok || decoded.value === undefined) {
      return { ok: false, error: decoded.error ?? `checkedRanges[${i}]: decode failed` }
    }
    checkedRanges.push(decoded.value)
  }
  return {
    ok: true,
    value: {
      textRevision: textRevision,
      suggestions: suggestions,
      checkedRanges: checkedRanges
    }
  }
}

export function decodeCorrectionResourceStatus(data: Object): DecodeResult<CorrectionResourceStatusDto> {
  if (!isRecord(data)) {
    return { ok: false, error: 'data: expected CorrectionResourceStatusDto' }
  }
  const record = data as Record<string, Object>
  const modelConfigured = record['modelConfigured']
  const modelFileAvailable = record['modelFileAvailable']
  const bundledLexiconAvailable = record['bundledLexiconAvailable']
  const modelPath = record['modelPath']
  const installedDictionaryFiles = record['installedDictionaryFiles']
  const missingDictionaryFiles = record['missingDictionaryFiles']
  if (typeof modelConfigured !== 'boolean' || typeof modelFileAvailable !== 'boolean' ||
      typeof bundledLexiconAvailable !== 'boolean' ||
      (typeof modelPath !== 'string' && modelPath !== null) || !Array.isArray(installedDictionaryFiles) ||
      !Array.isArray(missingDictionaryFiles) ||
      !installedDictionaryFiles.every((name: Object) => typeof name === 'string') ||
      !missingDictionaryFiles.every((name: Object) => typeof name === 'string')) {
    return { ok: false, error: 'data: invalid CorrectionResourceStatusDto fields' }
  }
  return {
    ok: true,
    value: {
      modelConfigured: modelConfigured,
      modelFileAvailable: modelFileAvailable,
      bundledLexiconAvailable: bundledLexiconAvailable,
      modelPath: modelPath as string | null,
      installedDictionaryFiles: installedDictionaryFiles as string[],
      missingDictionaryFiles: missingDictionaryFiles as string[]
    }
  }
}

export function decodeCorrectionModelPathStatus(data: Object): DecodeResult<CorrectionModelPathStatusDto> {
  if (!isRecord(data)) {
    return { ok: false, error: 'data: expected CorrectionModelPathStatusDto' }
  }
  const record = data as Record<string, Object>
  const configured = record['configured']
  const inUse = record['inUse']
  const deletionEligible = record['deletionEligible']
  if (typeof configured !== 'boolean' || typeof inUse !== 'boolean' ||
      typeof deletionEligible !== 'boolean') {
    return { ok: false, error: 'data: invalid CorrectionModelPathStatusDto fields' }
  }
  return {
    ok: true,
    value: {
      configured: configured,
      inUse: inUse,
      deletionEligible: deletionEligible
    }
  }
}
