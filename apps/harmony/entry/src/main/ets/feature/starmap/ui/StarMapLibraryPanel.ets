// StarMapLibraryPanel — 星图素材库（Issue #1037）。
//
// 只展示一个作用域的库目录：全局库或当前作品库。条目是「库归属」，
// 点击只发出插入请求；真正的引用实例由画布调用 Core 创建，位置属于当前画布。
// 删除库条目不会删除共享本体；本体删除与引用保护仍由 Core 判断。

import { StarMapAssetReference, StarMapAssetRef, StarMapLibraryEntryView, StarMapLibraryFolder, StarMapLibraryView } from '../../../corebridge/dto/StarMapDtos'
import { IWriterCoreBridge } from '../../../corebridge/IWriterCoreBridge'
import { getBridge } from '../../../app/AppContext'
import { CompactTextDialogArgs, openCompactTextDialog } from '../../../ui/components/CompactDialogs'
import { DesignTokens } from '../../../ui/theme/DesignTokens'

@ComponentV2
export struct StarMapLibraryPanel {
  @Param @Watch('onScopeChanged') scopeJson: string = '{"kind":"global"}'
  @Param scopeLabel: string = '全局素材库'
  @Param otherScopeJson: string = ''
  @Param canInsert: boolean = true
  @Param onInsert: (target: StarMapAssetRef, title: string) => void =
    (_target: StarMapAssetRef, _title: string): void => {}

  @Local entries: StarMapLibraryEntryView[] = []
  @Local folders: StarMapLibraryFolder[] = []
  @Local errorMessage: string = ''
  @Local loading: boolean = false

  private bridge: IWriterCoreBridge = getBridge()

  aboutToAppear(): void {
    this.reload()
  }

  private onScopeChanged(): void {
    this.reload()
  }

  private async reload(): Promise<void> {
    if (this.scopeJson.length === 0) {
      this.entries = []
      this.folders = []
      return
    }
    this.loading = true
    this.errorMessage = ''
    try {
      const result = await this.bridge.listStarMapLibrary(this.scopeJson)
      if (result.success && result.data !== undefined) {
        const view: StarMapLibraryView = result.data
        this.entries = view.entries
        this.folders = view.folders
      } else {
        this.errorMessage = result.userMessage ?? '加载素材库失败'
      }
    } catch (e) {
      this.errorMessage = '加载素材库失败'
    } finally {
      this.loading = false
    }
  }

  private folderName(folderId: string | null): string {
    if (folderId === null) {
      return ''
    }
    for (const folder of this.folders) {
      if (folder.folderId === folderId) {
        return folder.name
      }
    }
    return ''
  }

  private rowTitle(item: StarMapLibraryEntryView): string {
    const folder = this.folderName(item.entry.parentFolderId)
    return folder.length > 0 ? `${folder} / ${item.title}` : item.title
  }

  private requestCreateFolder(): void {
    const args: CompactTextDialogArgs = {
      title: '新建素材文件夹',
      placeholder: '文件夹名称',
      initialText: '',
      confirmLabel: '创建',
      onConfirm: (text: string): void => {
        const name = text.trim()
        if (name.length === 0) {
          return
        }
        this.createFolder(name)
      }
    }
    openCompactTextDialog(this.getUIContext(), args)
  }

  private async createFolder(name: string): Promise<void> {
    const result = await this.bridge.createStarMapLibraryFolder(this.scopeJson, '', name)
    if (!result.success) {
      this.errorMessage = result.userMessage ?? '新建文件夹失败'
      return
    }
    await this.reload()
  }

  private async showReferences(target: StarMapAssetRef): Promise<void> {
    const result = await this.bridge.listStarMapAssetReferences(JSON.stringify(target))
    if (!result.success || result.data === undefined) {
      this.errorMessage = result.userMessage ?? '读取引用位置失败'
      return
    }
    const refs: StarMapAssetReference[] = result.data
    const lines: string[] = refs.map((ref: StarMapAssetReference): string =>
      `${ref.projectId ?? '全局'} / ${ref.starmapId} / ${ref.instanceId}`)
    this.errorMessage = lines.length > 0 ? lines.join('\n') : '暂无引用'
  }

  @Builder
  EntryRow(item: StarMapLibraryEntryView) {
    Row({ space: 8 }) {
      Text(item.entry.target.kind === 'starmap' ? '图' : '点')
        .fontSize(11)
        .fontColor($r('sys.color.font_secondary'))
        .width(20)
      Text(this.rowTitle(item))
        .fontSize(13)
        .fontColor($r('sys.color.font_primary'))
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .layoutWeight(1)
    }
    .width('100%')
    .height(40)
    .padding({ left: 8, right: 8 })
    .borderRadius(DesignTokens.radiusSm)
    .onClick(() => {
      if (this.canInsert) {
        this.onInsert(item.entry.target, item.title)
      }
    })
    .gesture(LongPressGesture().onAction(() => {
      this.showReferences(item.entry.target)
    }))
  }

  @Builder
  FolderRow(folder: StarMapLibraryFolder) {
    Row({ space: 6 }) {
      Text('▸')
        .fontSize(11)
        .fontColor($r('sys.color.icon_secondary'))
      Text(folder.name)
        .fontSize(12)
        .fontWeight(FontWeight.Medium)
        .fontColor($r('sys.color.font_secondary'))
        .maxLines(1)
    }
    .width('100%')
    .height(32)
    .padding({ left: 4, right: 4 })
  }

  build() {
    Column() {
      Row() {
        Text(this.scopeLabel)
          .fontSize(13)
          .fontWeight(FontWeight.Bold)
          .fontColor($r('sys.color.font_primary'))
          .layoutWeight(1)
        Button('+')
          .fontSize(16)
          .width(28)
          .height(28)
          .backgroundColor($r('sys.color.comp_background_gray'))
          .onClick(() => this.requestCreateFolder())
      }
      .width('100%')
      .height(36)

      if (this.loading) {
        Text('加载中…').fontSize(12).fontColor($r('sys.color.font_secondary')).height(32)
      } else if (this.errorMessage.length > 0) {
        Text(this.errorMessage)
          .fontSize(11)
          .fontColor($r('sys.color.warning'))
          .maxLines(4)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
      } else if (this.scopeJson.length === 0) {
        Text('选择作品后显示作品素材库')
          .fontSize(11)
          .fontColor($r('sys.color.font_secondary'))
      } else if (this.entries.length === 0 && this.folders.length === 0) {
        Text('暂无素材')
          .fontSize(11)
          .fontColor($r('sys.color.font_secondary'))
          .height(32)
      } else {
        List({ space: 2 }) {
          ForEach(this.folders, (folder: StarMapLibraryFolder) => {
            ListItem() {
              this.FolderRow(folder)
            }
          }, (folder: StarMapLibraryFolder) => folder.folderId)
          ForEach(this.entries, (item: StarMapLibraryEntryView) => {
            ListItem() {
              this.EntryRow(item)
            }
          }, (item: StarMapLibraryEntryView) => item.entry.entryId)
        }
        .width('100%')
        .layoutWeight(1)
      }
    }
    .width('100%')
    .height('100%')
    .padding(8)
    .alignItems(HorizontalAlign.Start)
  }
}
