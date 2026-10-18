import { common } from '@kit.AbilityKit'
import { fileIo, fileUri, picker } from '@kit.CoreFileKit'
import { ResultEnvelope } from '../../../corebridge/dto/ResultEnvelope'

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
    configureModel: (path: string) => Promise<ResultEnvelope<boolean>>,
    onProgress?: (message: string) => void): Promise<CorrectionResourceImportResult> {
    let destination = ''
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
      CorrectionResourceImporter.nextModelId++
      const modelName = `${MODEL_FILE_PREFIX}${Date.now()}-${CorrectionResourceImporter.nextModelId}.klm`
      destination = `${directory}/${modelName}`

      onProgress?.('正在将模型复制到应用私有目录…')
      await CorrectionResourceImporter.copyPickedUri(sourceUri, destination)
      const copied = fileIo.statSync(destination)
      if (!copied.isFile() || copied.size <= 0) {
        throw new Error('所选模型文件为空或无法读取')
      }

      onProgress?.('正在把模型路径交给 Core…')
      const result = await configureModel(destination)
      if (!result.success || result.data !== true) {
        CorrectionResourceImporter.removeIfPresent(destination)
        destination = ''
        return {
          status: 'failed',
          message: result.userMessage ?? result.errorCode ?? 'Core 未能保存模型路径'
        }
      }
      return {
        status: 'imported',
        message: '模型已配置；文件格式待首次校对验证',
        resourceName: modelName
      }
    } catch (e) {
      if (destination.length > 0) {
        CorrectionResourceImporter.removeIfPresent(destination)
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
