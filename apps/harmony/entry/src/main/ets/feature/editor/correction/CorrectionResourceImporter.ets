import { common } from '@kit.AbilityKit'
import { resourceManager } from '@kit.LocalizationKit'
import { fileIo, fileUri, picker } from '@kit.CoreFileKit'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { CorrectionModelPathStatusDto } from './CorrectionDtos'
import { NativeCorrectionBridge } from './NativeCorrectionBridge'

const LEXICON_FILES: string[] = [
  'word_freq.txt',
  'same_pinyin.txt',
  'same_stroke.txt',
  'custom_confusion.txt',
  'person_name.txt',
  'place_name.txt',
  'proper_name.txt',
  'ignored_words.txt'
]

const MODEL_FILE_PREFIX = 'sujian-correction-model-'

// Issue #958：内置中文 KenLM 模型随 entry HAP 打包，构建期由
// tools/build_harmony.sh 调 packaging/correction/fetch-model.sh 生成到
// resources/rawfile/。运行时由 native OH_ResourceManager 接口分块流式复制到
// 应用私有目录，这里只声明锁定文件的名称、字节数与 SHA-256。
const BUNDLED_MODEL_RAW_FILE = 'people2014corpus_chars.klm'
const BUNDLED_MODEL_SIZE = 147608010
const BUNDLED_MODEL_SHA256 = '1d4ad3975144619a5bddc4ac53fcf319ec27d37dccb4ba58674af3a068d835a9'
const BUNDLED_MODEL_DIRECTORY = 'correction-models'

export interface CorrectionResourceSnapshot {
  directory: string
  dictionaryFiles: string[]
  modelFiles: string[]
}

export interface CorrectionResourceImportResult {
  status: 'imported' | 'cancelled' | 'failed'
  message: string
  resourceName?: string
}

/**
 * Imports user-selected correction assets into Core's app-data directory.
 * The UIAbility picker grants access to the selected URI; files are copied into
 * the application sandbox before the URI is discarded.
 */
export class CorrectionResourceImporter {
  private static nextModelId: number = 0
  private static bundledModelBridge: NativeCorrectionBridge = new NativeCorrectionBridge()
  // 同一应用会话内已校验的沙箱模型路径；模型文件在 correction-models/ 下，
  // 不参与 correction-data/ 的旧模型清理，缓存不会失效。
  private static bundledModelPath: string = ''

  static inspect(appDataRoot: string): CorrectionResourceSnapshot {
    const directory = `${appDataRoot}/correction-data`
    const dictionaryFiles: string[] = []
    const modelFiles: string[] = []
    try {
      if (!fileIo.accessSync(directory)) {
        return { directory: directory, dictionaryFiles: dictionaryFiles, modelFiles: modelFiles }
      }
      const names = fileIo.listFileSync(directory)
      for (let i = 0; i < names.length; i++) {
        const name = names[i]
        const path = `${directory}/${name}`
        try {
          const stat = fileIo.statSync(path)
          if (!stat.isFile() || stat.size <= 0) {
            continue
          }
          if (LEXICON_FILES.indexOf(name) >= 0) {
            dictionaryFiles.push(name)
          } else if (name.toLowerCase().endsWith('.klm')) {
            modelFiles.push(name)
          }
        } catch (_e) {
          // Ignore entries that disappear while the resource snapshot is read.
        }
      }
    } catch (_e) {
      // A missing or unreadable target is reflected as zero available resources.
    }
    return { directory: directory, dictionaryFiles: dictionaryFiles, modelFiles: modelFiles }
  }

  static async importModel(context: common.UIAbilityContext, appDataRoot: string,
    activateCandidate: (path: string) => Promise<ResultEnvelope<boolean>>,
    onProgress?: (message: string) => void): Promise<CorrectionResourceImportResult> {
    let stagingPath = ''
    let candidatePath = ''
    let candidateCreated = false
    let candidateActivated = false
    try {
      onProgress?.('正在打开系统文件选择器…')
      const options = new picker.DocumentSelectOptions()
      options.fileSuffixFilters = ['KenLM 模型（.klm）|.klm']
      options.maxSelectNumber = 1
      const uris = await new picker.DocumentViewPicker(context).select(options)
      if (uris.length === 0) {
        return { status: 'cancelled', message: '已取消模型选择' }
      }
      const sourceUri = uris[0]
      const selectedName = new fileUri.FileUri(sourceUri).name
      if (!selectedName.toLowerCase().endsWith('.klm')) {
        return { status: 'failed', message: '请选择 .klm 格式的 KenLM 模型文件' }
      }

      const directory = `${appDataRoot}/correction-data`
      CorrectionResourceImporter.ensureDirectory(directory)
      let modelName = ''
      do {
        CorrectionResourceImporter.nextModelId++
        const uniqueSuffix = `${Date.now()}-${CorrectionResourceImporter.nextModelId}`
        modelName = `${MODEL_FILE_PREFIX}${uniqueSuffix}.klm`
        candidatePath = `${directory}/${modelName}`
        stagingPath = `${directory}/.${modelName}.importing`
      } while (fileIo.accessSync(candidatePath) || fileIo.accessSync(stagingPath))

      onProgress?.('正在将模型复制到应用私有暂存目录…')
      await CorrectionResourceImporter.copyPickedUri(sourceUri, stagingPath)
      const copied = fileIo.statSync(stagingPath)
      if (!copied.isFile() || copied.size <= 0) {
        throw new Error('所选模型文件为空或无法读取')
      }

      const duplicatePath = await CorrectionResourceImporter.findDuplicateModel(directory, stagingPath, copied.size)
      if (duplicatePath.length > 0) {
        CorrectionResourceImporter.removeIfPresent(stagingPath)
        stagingPath = ''
        candidatePath = duplicatePath
      } else {
        await fileIo.moveFile(stagingPath, candidatePath)
        stagingPath = ''
        candidateCreated = true
      }

      onProgress?.('正在让 Core 校验并切换到候选模型…')
      const result = await activateCandidate(candidatePath)
      if (!result.success || result.data !== true) {
        if (candidateCreated) {
          CorrectionResourceImporter.removeIfPresent(candidatePath)
          candidateCreated = false
        }
        return {
          status: 'failed',
          message: result.userMessage ?? result.errorCode ?? 'Core 未能校验并切换模型'
        }
      }
      candidateActivated = true
      return {
        status: 'imported',
        message: duplicatePath.length > 0
          ? '发现内容相同的模型文件；Core 已校验并切换到现有文件'
          : 'Core 已校验并切换到新模型；旧模型保留以供生命周期检查',
        resourceName: candidatePath.substring(candidatePath.lastIndexOf('/') + 1)
      }
    } catch (e) {
      if (stagingPath.length > 0) {
        CorrectionResourceImporter.removeIfPresent(stagingPath)
      }
      if (candidateCreated && !candidateActivated) {
        CorrectionResourceImporter.removeIfPresent(candidatePath)
      }
      return { status: 'failed', message: `模型导入失败：${String(e)}` }
    }
  }

  static async importDictionary(context: common.UIAbilityContext, appDataRoot: string,
    onProgress?: (message: string) => void): Promise<CorrectionResourceImportResult> {
    let stagingPath = ''
    try {
      onProgress?.('正在打开系统文件选择器…')
      const options = new picker.DocumentSelectOptions()
      options.fileSuffixFilters = ['Core 词典资源（.txt）|.txt']
      options.maxSelectNumber = 1
      const uris = await new picker.DocumentViewPicker(context).select(options)
      if (uris.length === 0) {
        return { status: 'cancelled', message: '已取消词典选择' }
      }

      const sourceUri = uris[0]
      const selectedName = new fileUri.FileUri(sourceUri).name
      const canonicalName = CorrectionResourceImporter.canonicalLexiconName(selectedName)
      if (canonicalName.length === 0) {
        return {
          status: 'failed',
          message: `不支持“${selectedName}”；请使用 Core 约定的八种词典文件名`
        }
      }

      const directory = `${appDataRoot}/correction-data`
      CorrectionResourceImporter.ensureDirectory(directory)
      const destination = `${directory}/${canonicalName}`
      stagingPath = `${directory}/.${canonicalName}.${Date.now()}.importing`
      onProgress?.(`正在导入 ${canonicalName}…`)
      await CorrectionResourceImporter.copyPickedUri(sourceUri, stagingPath)
      const copied = fileIo.statSync(stagingPath)
      if (!copied.isFile() || copied.size <= 0) {
        throw new Error('所选词典文件为空或无法读取')
      }
      await fileIo.moveFile(stagingPath, destination, 0)
      stagingPath = ''
      return {
        status: 'imported',
        message: `${canonicalName} 已写入；Core 会在下次校对时重新读取`,
        resourceName: canonicalName
      }
    } catch (e) {
      if (stagingPath.length > 0) {
        CorrectionResourceImporter.removeIfPresent(stagingPath)
      }
      return { status: 'failed', message: `词典导入失败：${String(e)}` }
    }
  }

  /**
   * Issue #958: 把 HAP 内置 KenLM 模型复制到 `${appDataRoot}/correction-models/`。
   * 目录先创建；native 侧按固定缓冲分块流式复制并校验字节数与 SHA-256，
   * 返回校验后的沙箱绝对路径；任何失败都 reject，由调用方决定是否只记 warning。
   * 用户导入的模型始终优先，本方法只在调用方确认 Core 未配置模型时使用。
   */
  static async ensureBundledModel(manager: resourceManager.ResourceManager, appDataRoot: string): Promise<string> {
    if (CorrectionResourceImporter.bundledModelPath.length > 0) {
      return CorrectionResourceImporter.bundledModelPath
    }
    const directory = `${appDataRoot}/${BUNDLED_MODEL_DIRECTORY}`
    CorrectionResourceImporter.ensureDirectory(directory)
    const destination = `${directory}/${BUNDLED_MODEL_RAW_FILE}`
    const path = await CorrectionResourceImporter.bundledModelBridge.prepareBundledCorrectionModel(
      manager, BUNDLED_MODEL_RAW_FILE, destination, BUNDLED_MODEL_SIZE, BUNDLED_MODEL_SHA256)
    CorrectionResourceImporter.bundledModelPath = path
    return path
  }

  static removeModelIfEligible(appDataRoot: string, path: string,
    status: CorrectionModelPathStatusDto): boolean {
    if (!status.deletionEligible || status.configured || status.inUse || path.length === 0) {
      return false
    }
    const root = appDataRoot.endsWith('/') ? appDataRoot.substring(0, appDataRoot.length - 1) : appDataRoot
    const directoryPrefix = `${root}/correction-data/`
    if (!path.startsWith(directoryPrefix)) {
      return false
    }
    const name = path.substring(directoryPrefix.length)
    if (name.indexOf('/') >= 0 || !name.startsWith(MODEL_FILE_PREFIX) ||
        !name.toLowerCase().endsWith('.klm')) {
      return false
    }
    try {
      const stat = fileIo.statSync(path)
      if (!stat.isFile()) {
        return false
      }
      fileIo.unlinkSync(path)
      return true
    } catch (_e) {
      return false
    }
  }

  static isManagedModelFile(name: string): boolean {
    return name.startsWith(MODEL_FILE_PREFIX) && name.toLowerCase().endsWith('.klm')
  }

  private static canonicalLexiconName(name: string): string {
    const normalized = name.toLowerCase()
    for (let i = 0; i < LEXICON_FILES.length; i++) {
      if (LEXICON_FILES[i].toLowerCase() === normalized) {
        return LEXICON_FILES[i]
      }
    }
    return ''
  }

  private static ensureDirectory(path: string): void {
    if (!fileIo.accessSync(path)) {
      fileIo.mkdirSync(path, true)
    }
  }

  private static async copyPickedUri(uri: string, destination: string): Promise<void> {
    const source = fileIo.openSync(uri, fileIo.OpenMode.READ_ONLY)
    try {
      await fileIo.copyFile(source.fd, destination)
    } finally {
      fileIo.closeSync(source)
    }
  }

  private static async findDuplicateModel(directory: string, candidatePath: string,
    candidateSize: number): Promise<string> {
    let names: string[] = []
    try {
      names = fileIo.listFileSync(directory)
    } catch (_e) {
      return ''
    }
    for (let i = 0; i < names.length; i++) {
      const name = names[i]
      if (!name.toLowerCase().endsWith('.klm')) {
        continue
      }
      const path = `${directory}/${name}`
      try {
        const stat = fileIo.statSync(path)
        if (stat.isFile() && stat.size === candidateSize &&
            await CorrectionResourceImporter.filesEqual(candidatePath, path, candidateSize)) {
          return path
        }
      } catch (_e) {
        // Ignore a model that disappears or becomes unreadable during comparison.
      }
    }
    return ''
  }

  private static async filesEqual(leftPath: string, rightPath: string, size: number): Promise<boolean> {
    const left = fileIo.openSync(leftPath, fileIo.OpenMode.READ_ONLY)
    let right: fileIo.File | null = null
    try {
      right = fileIo.openSync(rightPath, fileIo.OpenMode.READ_ONLY)
      let offset = 0
      const chunkSize = 256 * 1024
      while (offset < size) {
        const length = Math.min(chunkSize, size - offset)
        const leftBytes = await CorrectionResourceImporter.readChunk(left.fd, length)
        const rightBytes = await CorrectionResourceImporter.readChunk(right.fd, length)
        if (leftBytes === null || rightBytes === null) {
          return false
        }
        for (let i = 0; i < length; i++) {
          if (leftBytes[i] !== rightBytes[i]) {
            return false
          }
        }
        offset += length
      }
      return true
    } finally {
      if (right !== null) {
        fileIo.closeSync(right)
      }
      fileIo.closeSync(left)
    }
  }

  private static async readChunk(fd: number, length: number): Promise<Uint8Array | null> {
    const bytes = new Uint8Array(length)
    let offset = 0
    while (offset < length) {
      const remaining = length - offset
      const buffer = new ArrayBuffer(remaining)
      const read = await fileIo.read(fd, buffer)
      if (read <= 0) {
        return null
      }
      bytes.set(new Uint8Array(buffer, 0, read), offset)
      offset += read
    }
    return bytes
  }

  private static removeIfPresent(path: string): void {
    try {
      if (fileIo.accessSync(path)) {
        fileIo.unlinkSync(path)
      }
    } catch (_e) {
      // Cleanup failure must not hide the original import or Core error.
    }
  }
}
