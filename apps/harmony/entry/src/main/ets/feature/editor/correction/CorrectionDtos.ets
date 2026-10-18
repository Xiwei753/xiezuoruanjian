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

export interface CorrectionResourceStatusDto {
  modelConfigured: boolean
  modelFileAvailable: boolean
  bundledLexiconAvailable: boolean
  installedDictionaryFiles: string[]
  missingDictionaryFiles: string[]
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

export function decodeCorrectionSuggestions(data: Object): DecodeResult<CorrectionSuggestionDto[]> {
  if (!Array.isArray(data)) {
    return { ok: false, error: 'data: expected CorrectionSuggestionDto[]' }
  }
  const suggestions: CorrectionSuggestionDto[] = []
  for (let i = 0; i < data.length; i++) {
    const decoded = decodeSuggestion(data[i], i)
    if (!decoded.ok || decoded.value === undefined) {
      return { ok: false, error: decoded.error ?? `suggestions[${i}]: decode failed` }
    }
    suggestions.push(decoded.value)
  }
  return { ok: true, value: suggestions }
}

export function decodeCorrectionResourceStatus(data: Object): DecodeResult<CorrectionResourceStatusDto> {
  if (!isRecord(data)) {
    return { ok: false, error: 'data: expected CorrectionResourceStatusDto' }
  }
  const record = data as Record<string, Object>
  const modelConfigured = record['modelConfigured']
  const modelFileAvailable = record['modelFileAvailable']
  const bundledLexiconAvailable = record['bundledLexiconAvailable']
  const installedDictionaryFiles = record['installedDictionaryFiles']
  const missingDictionaryFiles = record['missingDictionaryFiles']
  if (typeof modelConfigured !== 'boolean' || typeof modelFileAvailable !== 'boolean' ||
      typeof bundledLexiconAvailable !== 'boolean' || !Array.isArray(installedDictionaryFiles) ||
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
      installedDictionaryFiles: installedDictionaryFiles as string[],
      missingDictionaryFiles: missingDictionaryFiles as string[]
    }
  }
}
