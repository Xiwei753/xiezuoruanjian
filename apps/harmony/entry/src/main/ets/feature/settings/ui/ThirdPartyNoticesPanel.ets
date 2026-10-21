/**
 * 内置校对模型「第三方资源」面板（Issue #958）。
 *
 * 展示随 HAP 打包的内置中文 KenLM 模型来源声明（THIRD_PARTY_MODEL_NOTICES.md）
 * 与完整许可文本（BLOOM-RAIL-1.0.txt）。两份文本由 entry 模块的 Hvigor 构建
 * 钩子在资源打包前写入 resources/rawfile/（见 entry/hvigorfile.ts），
 * 与 people2014corpus_chars.klm 一起分发；此处只读取小体积文本，不触碰模型本体。
 */
import { UIContext, ComponentContent } from '@kit.ArkUI'
import util from '@ohos.util'
import hilog from '@ohos.hilog'
import { DesignTokens } from '../../../ui/theme/DesignTokens'

const TAG = 'ThirdPartyNoticesPanel'
const DOMAIN = 0x0000

/** rawfile 中的声明 / 许可文件名（构建期生成，见 hvigorfile.ts）。 */
const NOTICES_RAW_FILE = 'THIRD_PARTY_MODEL_NOTICES.md'
const LICENSE_RAW_FILE = 'BLOOM-RAIL-1.0.txt'

/** 面板参数：关闭回调由调用方处理。 */
export interface ThirdPartyNoticesPanelArgs {
  onRequestClose: () => void
}

@ComponentV2
export struct ThirdPartyNoticesPanel {
  @Param onRequestClose: () => void = () => {}
  @Local noticesText: string = ''
  @Local licenseText: string = ''
  @Local loadFailed: boolean = false

  aboutToAppear(): void {
    try {
      const decoder = new util.TextDecoder('utf-8')
      const manager = getContext(this).resourceManager
      this.noticesText = decoder.decodeWithStream(manager.getRawFileContentSync(NOTICES_RAW_FILE))
      this.licenseText = decoder.decodeWithStream(manager.getRawFileContentSync(LICENSE_RAW_FILE))
    } catch (e) {
      this.loadFailed = true
      hilog.error(DOMAIN, TAG, 'read bundled notices failed: %{public}s', String(e))
    }
  }

  build() {
    Row() {
      Column() {
        Row() {
          Text($r('app.string.third_party_notices_title'))
            .fontSize(20)
            .fontWeight(FontWeight.Bold)

          Blank()

          Button($r('app.string.third_party_notices_close'), { type: ButtonType.Normal })
            .height(32)
            .fontSize(13)
            .fontColor($r('sys.color.font_primary'))
            .backgroundColor($r('sys.color.comp_background_gray'))
            .borderRadius(DesignTokens.radiusSm)
            .onClick(() => { this.onRequestClose() })
        }
        .width('100%')
        .margin({ bottom: 12 })

        if (this.loadFailed) {
          Text($r('app.string.third_party_notices_load_failed'))
            .fontSize(14)
            .width('100%')
        } else {
          Scroll() {
            Column() {
              Text(this.noticesText)
                .fontSize(13)
                .width('100%')

              Divider()
                .margin({ top: 16, bottom: 16 })

              Text(this.licenseText)
                .fontSize(13)
                .width('100%')
            }
            .width('100%')
            .alignItems(HorizontalAlign.Start)
          }
          .layoutWeight(1)
          .width('100%')
        }
      }
      .width('100%')
      .height('100%')
      .constraintSize({ maxWidth: 960 })
      .padding(20)
      .backgroundColor($r('sys.color.background_secondary'))
      .borderRadius(DesignTokens.dialogRadius)
    }
    .width('100%')
    .height('84%')
    .padding({ left: 24, right: 24 })
    .justifyContent(FlexAlign.Center)
  }
}

/** 文件级 @Builder 函数：把 ComponentContent 传来的参数转交给组件。 */
@Builder
function ThirdPartyNoticesPanelBuilder(args: ThirdPartyNoticesPanelArgs) {
  ThirdPartyNoticesPanel({ onRequestClose: args.onRequestClose })
}

/**
 * 构造第三方资源面板的 ComponentContent。
 * 调用方用 getUIContext().getPromptAction().openCustomDialog(content, {...}) 打开，
 * 关闭时用同一个 content 调用 closeCustomDialog。
 *
 * @param uiContext 调用方的 UIContext
 * @param args 面板参数
 * @returns 可用于 openCustomDialog / closeCustomDialog 的 ComponentContent
 */
export function createThirdPartyNoticesPanelContent(
  uiContext: UIContext,
  args: ThirdPartyNoticesPanelArgs
): ComponentContent<ThirdPartyNoticesPanelArgs> {
  return new ComponentContent<ThirdPartyNoticesPanelArgs>(uiContext, wrapBuilder(ThirdPartyNoticesPanelBuilder), args)
}
