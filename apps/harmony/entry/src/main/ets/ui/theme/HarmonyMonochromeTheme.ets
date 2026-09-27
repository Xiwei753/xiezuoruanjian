import { CustomColors, CustomTheme, ThemeControl } from '@kit.ArkUI'

class SujianMonochromeColors implements CustomColors {
  brand: ResourceColor
  fontEmphasize: ResourceColor
  iconEmphasize: ResourceColor
  backgroundEmphasize: ResourceColor
  compBackgroundEmphasize: ResourceColor
  interactiveSelect: ResourceColor
  fontOnPrimary: ResourceColor
  iconOnPrimary: ResourceColor

  constructor(isDark: boolean) {
    const emphasize: ResourceColor = isDark ? Color.White : Color.Black
    const onEmphasize: ResourceColor = isDark ? Color.Black : Color.White

    this.brand = emphasize
    this.fontEmphasize = emphasize
    this.iconEmphasize = emphasize
    this.backgroundEmphasize = emphasize
    this.compBackgroundEmphasize = emphasize
    this.interactiveSelect = emphasize
    this.fontOnPrimary = onEmphasize
    this.iconOnPrimary = onEmphasize
  }
}

class SujianMonochromeTheme implements CustomTheme {
  colors?: CustomColors

  constructor(isDark: boolean) {
    this.colors = new SujianMonochromeColors(isDark)
  }
}

export function applySujianMonochromeTheme(isDark: boolean): void {
  ThemeControl.setDefaultTheme(new SujianMonochromeTheme(isDark))
}
