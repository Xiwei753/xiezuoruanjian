import { getAppContext, getAppDataRoot } from '../../../app/AppContext'
import { DesignTokens } from '../../../ui/theme/DesignTokens'
import { CorrectionResourceImporter } from '../../editor/correction/CorrectionResourceImporter'
import { NativeCorrectionBridge } from '../../editor/correction/NativeCorrectionBridge'

/**
 * Correction resource management lives with the other editor settings.
 * Importing a model or dictionary changes only local files; the writing flow keeps
 * a lightweight proofing entry and reads Core's current resource status when needed.
 */
@ComponentV2
export struct CorrectionResourceSettingsPanel {
  @Local resourceDirectory: string = ''
  @Local modelStatus: string = '正在读取模型状态…'
  @Local dictionaryStatus: string = '正在读取词典状态…'
  @Local isBusy: boolean = false
  @Local progressMessage: string = ''
  @Local errorMessage: string = ''

  private bridge: NativeCorrectionBridge = new NativeCorrectionBridge()

  aboutToAppear(): void {
    void this.refreshStatus()
  }

  private async refreshStatus(): Promise<void> {
    const appDataRoot: string = getAppDataRoot()
    if (appDataRoot.length === 0) {
      this.resourceDirectory = ''
      this.errorMessage = '无法获取应用数据目录。'
      this.modelStatus = '模型状态不可用'
      this.dictionaryStatus = '词典状态不可用'
      return
    }
    const snapshot = CorrectionResourceImporter.inspect(appDataRoot)
    this.resourceDirectory = snapshot.directory
    const result = await this.bridge.getResourceStatus()
    if (!result.success || result.data === undefined) {
      const message = result.userMessage ?? result.rawError ?? '未知错误'
      this.modelStatus = `无法读取 Core 模型状态：${message}`
      this.dictionaryStatus = `无法读取 Core 词典状态：${message}`
      return
    }
    const status = result.data
    const missingDictionaries = status.missingDictionaryFiles
    const dictionaryReadiness = missingDictionaries.length > 0
      ? `已安装 ${status.installedDictionaryFiles.length}/8；未安装：${missingDictionaries.join('、')}`
      : '六种可选词典均已安装'
    this.dictionaryStatus = status.bundledLexiconAvailable
      ? `Core 内置词库已就绪；${dictionaryReadiness}`
      : 'Core 内置词库不可用'
    const modelName = status.modelPath === null
      ? ''
      : status.modelPath.substring(status.modelPath.lastIndexOf('/') + 1)
    this.modelStatus = status.modelFileAvailable
      ? `已配置可读取的模型${modelName.length > 0 ? `：${modelName}` : '文件'}`
      : status.modelConfigured
        ? '已配置模型路径，但模型文件不可用'
        : '尚未配置语言模型；内置固定规则仍可用'
    this.errorMessage = ''
  }

  private async importModel(): Promise<void> {
    if (this.isBusy) {
      return
    }
    const context = getAppContext().getAbilityContext()
    const appDataRoot: string = getAppDataRoot()
    if (context === null || appDataRoot.length === 0) {
      this.errorMessage = '无法获取应用目录或 UIAbilityContext，模型未导入。'
      return
    }
    this.isBusy = true
    this.errorMessage = ''
    this.progressMessage = '正在准备导入模型…'
    try {
      const result = await CorrectionResourceImporter.importModel(
        context,
        appDataRoot,
        (path: string) => this.bridge.setModelPath(path),
        (message: string): void => { this.progressMessage = message }
      )
      await this.refreshStatus()
      if (result.status === 'imported') {
        this.modelStatus = result.message
      } else if (result.status === 'cancelled') {
        this.modelStatus = `模型配置未改变；${result.message}`
      } else {
        this.modelStatus = `${result.message}；${this.modelStatus}`
      }
    } catch (e) {
      this.errorMessage = `模型导入失败：${String(e)}`
    } finally {
      this.isBusy = false
      this.progressMessage = ''
    }
  }

  private async importDictionary(): Promise<void> {
    if (this.isBusy) {
      return
    }
    const context = getAppContext().getAbilityContext()
    const appDataRoot: string = getAppDataRoot()
    if (context === null || appDataRoot.length === 0) {
      this.errorMessage = '无法获取应用目录或 UIAbilityContext，词典未导入。'
      return
    }
    this.isBusy = true
    this.errorMessage = ''
    this.progressMessage = '正在准备导入词典…'
    try {
      const result = await CorrectionResourceImporter.importDictionary(
        context,
        appDataRoot,
        (message: string): void => { this.progressMessage = message }
      )
      await this.refreshStatus()
      this.dictionaryStatus = `${result.message}；${this.dictionaryStatus}`
    } catch (e) {
      this.errorMessage = `词典导入失败：${String(e)}`
    } finally {
      this.isBusy = false
      this.progressMessage = ''
    }
  }

  @Builder
  ResourceAction(label: ResourceStr, onTriggered: () => void) {
    Button(label, { type: ButtonType.Normal })
      .width('100%')
      .height(40)
      .fontSize(14)
      .fontColor($r('sys.color.font_primary'))
      .backgroundColor($r('sys.color.ohos_id_color_button_normal'))
      .borderRadius(DesignTokens.radiusSm)
      .enabled(!this.isBusy)
      .onClick(onTriggered)
  }

  build() {
    Column({ space: 8 }) {
      Text('校对资源')
        .fontSize(16)
        .fontWeight(FontWeight.Medium)
        .width('100%')
      Text('模型和词典只在本机离线校对时使用；写作页的“校对”入口只展示检查结果。')
        .fontSize(12)
        .fontColor($r('sys.color.font_secondary'))
        .width('100%')

      Column() {
        this.ResourceAction('导入 ONNX 模型目录', (): void => { void this.importModel() })
      }
      .width('100%')
      .margin({ top: 4 })

      this.ResourceAction('导入词典 .txt', (): void => { void this.importDictionary() })

      if (this.isBusy && this.progressMessage.length > 0) {
        Row({ space: 8 }) {
          LoadingProgress()
            .width(16)
            .height(16)
          Text(this.progressMessage)
            .fontSize(12)
            .fontColor($r('sys.color.font_secondary'))
            .layoutWeight(1)
        }
        .width('100%')
      }

      if (this.modelStatus.length > 0) {
        Text(this.modelStatus)
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))
          .width('100%')
      }
      if (this.dictionaryStatus.length > 0) {
        Text(this.dictionaryStatus)
          .fontSize(12)
          .fontColor($r('sys.color.font_secondary'))
          .width('100%')
      }
      if (this.resourceDirectory.length > 0) {
        Column({ space: 2 }) {
          Text('资源位置')
            .fontSize(12)
            .fontColor($r('sys.color.font_tertiary'))
            .width('100%')
          Text(this.resourceDirectory)
            .fontSize(11)
            .fontColor($r('sys.color.font_tertiary'))
            .width('100%')
        }
        .width('100%')
      }
      if (this.errorMessage.length > 0) {
        Text(this.errorMessage)
          .fontSize(12)
          .fontColor($r('sys.color.ohos_id_color_alert'))
          .width('100%')
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
    .padding({ top: 8 })
  }
}
