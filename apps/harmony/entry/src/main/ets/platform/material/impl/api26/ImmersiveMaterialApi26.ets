// ImmersiveMaterialApi26 — API26+ 沉浸式系统材质（uiMaterial.ImmersiveMaterial）。
//
// 页面内容流里的普通组件挂上这个材质，ArkUI 会打
// `Material inactive: out of scope. Use component in navigation title bar or Tabbar.`
// 但「在导航标题栏的自绘区域（titleBar.content.stackBuilder）里」是否属于生效范围，
// 必须实机验证，不能靠推断 —— 结论见 platform/API_MATRIX.md
// 「沉浸式系统材质的生效范围」一节。
//
// 材质参数按 SDK 声明的默认值取舍（见 @ohos.arkui.uiMaterial.d.ts 的构造函数注释，
// 默认值是 style:REGULAR / materialColor:Color.Transparent / colorInvert:false /
// applyShadow:true / interactive:false / lightEffect:undefined）：
//   - style 取 THIN：官方《沉浸光感常见问题》示例的标题栏写法。
//     ImmersiveStyle 是官方说的「材质厚薄程度」（模糊程度、高光效果），
//     ULTRA_THIN 最薄、THIN 次之、REGULAR 最厚（也是默认值）。
//   - materialColor **不给**（默认 Transparent）。之前传 '#33FFFFFF' 想「保留通透度」，
//     实际是把一层白纱盖在材质上，胶囊糊成一片发灰起雾的乳白，和底栏差很远。
//     官方参数要点就是 materialColor 必须透明，给了不透明纯色会把材质滤镜完全挡住。
//   - colorInvert 不给（默认 false）：它管的是材质层内容的自动反色，
//     我们前景色已经用系统可反色资源 sys.color.icon_primary，不需要它。
//   - interactive / lightEffect 打开：按压那圈跟手的光靠这两个，
//     默认关闭。lightEffect.color 只指定光的颜色，不影响材质厚薄。
//   - applyShadow 用默认 true：阴影交给材质自己出。
// 开了材质后不要再给组件设 backgroundColor / 背景模糊 / border，
// 那些属性会盖在材质层之上。

import { uiMaterial } from '@kit.ArkUI'

export class ImmersiveMaterialApi26 {
  private static instance: ImmersiveMaterialApi26

  static getInstance(): ImmersiveMaterialApi26 {
    if (!ImmersiveMaterialApi26.instance) {
      ImmersiveMaterialApi26.instance = new ImmersiveMaterialApi26()
    }
    return ImmersiveMaterialApi26.instance
  }

  isSupported(): boolean {
    return uiMaterial.isImmersiveMaterialSupported()
  }

  // applyShadow 决定阴影归谁：true 用材质自带的阴影，false 关掉材质阴影、
  // 由调用方自己设 shadow 通用属性。官方《沉浸式系统材质视效》原话：
  // 「沉浸式系统材质默认自带阴影效果（applyShadow 为 true），优先于 shadow 通用属性，
  //   此时自定义的 shadow 设置不会生效。如需使用自定义阴影，将 applyShadow 置为 false
  //   后再设置 shadow，沉浸式系统材质的阴影效果即不生效。」
  // 这是官方唯一明文允许的替换通道；border 没有对应的替换通道（材质接管边框，
  // 自定义 border 会盖在材质层上把高光挡住）。
  buildFloatingMaterial(applyShadow: boolean = true): uiMaterial.ImmersiveMaterial {
    return new uiMaterial.ImmersiveMaterial({
      style: uiMaterial.ImmersiveStyle.THIN,
      // interactive 与 lightEffect 是按压那圈光的来源，两者默认值分别是
      // false / undefined，不显式打开就没有按压反馈（实测去掉后按住胶囊表面恒定 20 级）。
      interactive: true,
      lightEffect: {
        color: Color.White
      },
      applyShadow: applyShadow
    })
  }
}
