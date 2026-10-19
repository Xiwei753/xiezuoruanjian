# 内置中文语言模型第三方声明

素笺写作的 Linux Qt（RPM）、Android（APK）、HarmonyOS（HAP）发行包内置
KenLM 中文语言模型 `people2014corpus_chars.klm`。该文件来自上游模型仓库，
不是素笺项目自制的模型，其许可与素笺软件的 GPL-3.0-only 相互独立。本声明与
`crates/sujian-proof/THIRD_PARTY_NOTICES.md`、`docs/correction_model_assets.md`
共同构成安装包内的第三方来源与许可说明。

## 模型文件

- 文件名：`people2014corpus_chars.klm`
- 上游模型仓库：<https://huggingface.co/shibing624/chinese-kenlm-klm>
- 发布者：shibing624（许明 / Ming Xu），pycorrector 项目作者
- 固定下载地址：<https://huggingface.co/shibing624/chinese-kenlm-klm/resolve/main/people2014corpus_chars.klm>
- 大小：147,608,010 字节
- SHA-256：`1d4ad3975144619a5bddc4ac53fcf319ec27d37dccb4ba58674af3a068d835a9`
- 上游页面许可标注：`bigscience-bloom-rail-1.0`（BigScience RAIL License v1.0）
- 训练语料（上游说明）：2014 版人民日报语料；见 pycorrector README 的
  “Language Model” 一节：<https://github.com/shibing624/pycorrector#language-model>

素笺按上述字节数与 SHA-256 逐字节校验后原样打包，不修改模型文件内容。

## 许可与使用限制

模型按其上游页面标注的 BigScience RAIL License v1.0 分发：

- 许可完整原文副本：本目录 `BLOOM-RAIL-1.0.txt`；固定页面：
  <https://huggingface.co/spaces/bigscience/license>。
- 再分发条件（RAIL 第 III 节）：向模型接收方提供本许可副本；保留版权、专利、
  商标与归属声明；对修改过的文件显著标注；“使用限制”必须作为可执行条款告知
  后续用户。
- 使用限制（Attachment A）概述：不得用于违法用途；不得用于侵害未成年人；不得
  生成或传播蓄意伤害他人的虚假信息；不得生成或传播可用于伤害个人的身份信息；
  不得在未明确标注机器生成的情况下生成或传播内容；不得诽谤、贬损或骚扰他人；
  不得冒充他人；不得用于对个人法律权利产生不利影响的完全自动化决策；不得用于
  歧视性用途；不得利用特定群体的弱点进行操纵；不得提供医疗建议与诊断；不得
  用于司法、执法、移民或庇护程序中的预测性用途。完整条款以 `BLOOM-RAIL-1.0.txt`
  原文为准。

## 相关软件来源（与模型文件分开）

- pycorrector（纠错算法参考与模型发布项目）：Apache-2.0，作者 Ming Xu
  （shibing624），<https://github.com/shibing624/pycorrector>。素笺的 Rust 移植
  文件清单与修改说明见 `crates/sujian-proof/THIRD_PARTY_NOTICES.md`；发行包不
  包含 pycorrector 或其 Python 运行时。
- KenLM（模型文件格式与查询算法来源）：<https://github.com/kpu/kenlm>，按文件
  的 LGPL-2.1-or-later 许可与例外处理，细节见
  `crates/sujian-proof/THIRD_PARTY_NOTICES.md`；素笺在 Rust 中自行读取模型并
  查询，不链接 KenLM C++ 库。

用户自行导入的其他 `.klm` 模型不受本声明约束，其许可由用户与对应来源决定；
素笺只读取本地模型文件，不上传任何模型或正文内容。
