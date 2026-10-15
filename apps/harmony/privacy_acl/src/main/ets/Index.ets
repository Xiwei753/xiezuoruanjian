// privacy_acl — DLP 防窥受限能力模块（HSP）的公开入口。
// 仅在 acl product 下装配；普通 default product 不依赖本模块。
// re-export 各 API Level 的 DLP 防窥 facade，供 acl target 的 ShoulderSurfingProvider 调用。
export { DlpAntiPeepApi20 } from './impl/api20/DlpAntiPeepApi20'
export { DlpAntiPeepApi23 } from './impl/api23/DlpAntiPeepApi23'
