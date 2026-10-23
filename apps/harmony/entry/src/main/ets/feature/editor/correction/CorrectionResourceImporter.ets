import { common } from '@kit.AbilityKit'
import { resourceManager } from '@kit.LocalizationKit'
import { fileIo, fileUri, picker } from '@kit.CoreFileKit'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'
import { CorrectionModelPathStatusDto } from './CorrectionDtos'
import { NativeCorrectionBridge } from './NativeCorrectionBridge'

const LEXICON_FILES: string[] = [
  'word_freq.txt',
  'custom_confusion.txt',
  'person_name.txt',
  'place_name.txt',
  'proper_name.txt',
  'ignored_words.txt'
]

const MODEL_FILE_PREFIX = 'sujian-correction-model-'
const MODEL_FILE_NAME = 'model.onnx'
const VOCAB_FILE_NAME = 'vocab.txt'
const MODEL_BUNDLE_FILES: string[] = [
  MODEL_FILE_NAME,
  VOCAB_FILE_NAME,
  'config.json',
  'tokenizer.json',
  'tokenizer_config.json',
  'special_tokens_map.json',
  'calibrator.json'
]

// Issue #985：内置 MacBERT4CSC FP32 ONNX 模型随 entry HAP 打包，构建期由
// tools/build_harmony.sh 调 packaging/correction/fetch-model.sh 生成到
// resources/rawfile/。运行时由 native OH_ResourceManager 接口分块流式复制到
// 应用私有目录，这里只声明锁定文件的名称、字节数与 SHA-256。
const BUNDLED_MODEL_DIRECTORY = 'correction-models'
const BUNDLED_BUNDLE_DIRECTORY = 'macbert4csc'
const BUNDLED_MODEL_RAW_FILE = 'macbert4csc/model.onnx'
const BUNDLED_MODEL_SIZE = 474336933
const BUNDLED_MODEL_SHA256 = 'a4d9d6807c0c8bc9d014462a728e7212315630c669a6bd670b653154391a73f3'
const BUNDLED_VOCAB_RAW_FILE = 'macbert4csc/vocab.txt'
const BUNDLED_VOCAB_SIZE = 109540
const BUNDLED_VOCAB_SHA256 = '45bbac6b341c319adc98a532532882e91a9cefc0329aa57bac9ae761c27b291c'

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
          if (stat.isDirectory() && CorrectionResourceImporter.isManagedModelFile(name)) {
            modelFiles.push(name)
            continue
          }
          if (!stat.isFile() || stat.size <= 0) {
            continue
          }
          if (LEXICON_FILES.indexOf(name) >= 0) {
            dictionaryFiles.push(name)
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
    let stagingDirectory = ''
    let candidateDirectory = ''
    let candidateCreated = false
    let candidateActivated = false
    try {
      onProgress?.('正在打开系统文件选择器…')
      const options = new picker.DocumentSelectOptions()
      options.fileSuffixFilters = ['ONNX 模型资源（.onnx/.txt/.json）|.onnx,.txt,.json']
      options.maxSelectNumber = MODEL_BUNDLE_FILES.length
      const uris = await new picker.DocumentViewPicker(context).select(options)
      if (uris.length === 0) {
        return { status: 'cancelled', message: '已取消模型选择' }
      }

      const directory = `${appDataRoot}/correction-data`
      CorrectionResourceImporter.ensureDirectory(directory)
      CorrectionResourceImporter.nextModelId++
      const uniqueSuffix = `${Date.now()}-${CorrectionResourceImporter.nextModelId}`
      stagingDirectory = `${directory}/.${MODEL_FILE_PREFIX}${uniqueSuffix}.importing`
      candidateDirectory = `${directory}/${MODEL_FILE_PREFIX}${uniqueSuffix}`
      CorrectionResourceImporter.ensureDirectory(stagingDirectory)

      onProgress?.('正在将模型资源复制到应用私有暂存目录…')
      let copiedModel = false
      let copiedVocab = false
      for (let i = 0; i < uris.length; i++) {
        const sourceUri = uris[i]
        const selectedName = new fileUri.FileUri(sourceUri).name
        if (MODEL_BUNDLE_FILES.indexOf(selectedName) < 0) {
          continue
        }
        const destination = `${stagingDirectory}/${selectedName}`
        await CorrectionResourceImporter.copyPickedUri(sourceUri, destination)
        const copied = fileIo.statSync(destination)
        if (!copied.isFile() || copied.size <= 0) {
          throw new Error(`模型资源 ${selectedName} 为空或无法读取`)
        }
        if (selectedName === MODEL_FILE_NAME) {
          copiedModel = true
        } else if (selectedName === VOCAB_FILE_NAME) {
          copiedVocab = true
        }
      }
      if (!copiedModel || !copiedVocab) {
        throw new Error('模型目录必须同时包含 model.onnx 与 vocab.txt')
      }

      await fileIo.moveDir(stagingDirectory, candidateDirectory)
      stagingDirectory = ''
      candidateCreated = true

      onProgress?.('正在让 Core 校验并切换到候选模型…')
      const result = await activateCandidate(candidateDirectory)
      if (!result.success || result.data !== true) {
        if (candidateCreated) {
          CorrectionResourceImporter.removeModelDirectory(candidateDirectory)
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
        message: 'Core 已校验并切换到新的 ONNX 模型目录；旧模型保留以供生命周期检查',
        resourceName: candidateDirectory.substring(candidateDirectory.lastIndexOf('/') + 1)
      }
    } catch (e) {
      if (stagingDirectory.length > 0) {
        CorrectionResourceImporter.removeModelDirectory(stagingDirectory)
      }
      if (candidateCreated && !candidateActivated) {
        CorrectionResourceImporter.removeModelDirectory(candidateDirectory)
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
          message: `不支持“${selectedName}”；请使用 Core 约定的六种词典文件名`
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
   * Issue #985: 把 HAP 内置 MacBERT4CSC ONNX 模型复制到
   * `${appDataRoot}/correction-models/macbert4csc/`。目录先创建；native 侧按固定
   * 缓冲分块流式复制并校验字节数与 SHA-256，返回校验后的沙箱目录；任何失败都
   * reject，由调用方决定是否只记 warning。用户导入的模型始终优先。
   */
  static async ensureBundledModel(manager: resourceManager.ResourceManager, appDataRoot: string): Promise<string> {
    if (CorrectionResourceImporter.bundledModelPath.length > 0) {
      return CorrectionResourceImporter.bundledModelPath
    }
    const directory = `${appDataRoot}/${BUNDLED_MODEL_DIRECTORY}/${BUNDLED_BUNDLE_DIRECTORY}`
    CorrectionResourceImporter.ensureDirectory(directory)
    await CorrectionResourceImporter.bundledModelBridge.prepareBundledCorrectionModel(
      manager, BUNDLED_MODEL_RAW_FILE, `${directory}/${MODEL_FILE_NAME}`,
      BUNDLED_MODEL_SIZE, BUNDLED_MODEL_SHA256)
    await CorrectionResourceImporter.bundledModelBridge.prepareBundledCorrectionModel(
      manager, BUNDLED_VOCAB_RAW_FILE, `${directory}/${VOCAB_FILE_NAME}`,
      BUNDLED_VOCAB_SIZE, BUNDLED_VOCAB_SHA256)
    CorrectionResourceImporter.bundledModelPath = directory
    return directory
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
    if (name.indexOf('/') >= 0 || !CorrectionResourceImporter.isManagedModelFile(name)) {
      return false
    }
    try {
      const stat = fileIo.statSync(path)
      if (stat.isDirectory()) {
        CorrectionResourceImporter.removeModelDirectory(path)
        return true
      }
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
    return name.startsWith(MODEL_FILE_PREFIX)
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

  private static removeModelDirectory(path: string): void {
    try {
      if (!fileIo.accessSync(path)) {
        return
      }
      const names = fileIo.listFileSync(path)
      for (let i = 0; i < names.length; i++) {
        fileIo.unlinkSync(`${path}/${names[i]}`)
      }
      fileIo.rmdirSync(path)
    } catch (_e) {
      // Cleanup failure must not hide the original import or Core error.
    }
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
