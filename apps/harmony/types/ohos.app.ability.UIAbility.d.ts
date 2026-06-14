declare module '@ohos.app.ability.UIAbility' {
  import { WindowStage } from '@ohos.window'
  export default class UIAbility {
    onCreate(want: object, launchParam: object): void
    onDestroy(): void
    onWindowStageCreate(windowStage: WindowStage): void
    onWindowStageDestroy(): void
    onForeground(): void
    onBackground(): void
  }
}
