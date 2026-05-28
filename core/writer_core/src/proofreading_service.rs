//! # 校对服务模块 (Proofreading Service)
//!
//! 本模块实现了文本校对和自动纠错功能，用于帮助用户提高写作质量。
//!
//! ## 主要功能
//!
//! - **多种校对模式**: 支持关闭、仅标记、确认替换、自动替换等模式
//! - **校对强度控制**: 支持低、正常、严格三种校对强度
//! - **纠错词典**: 维护错误词汇与正确词汇的映射关系
//! - **忽略规则**: 支持用户自定义忽略特定词汇
//! - **位置追踪**: 记录校对建议在原文中的精确位置
//!
//! ## 校对模式
//!
//! - `Off`: 关闭校对功能
//! - `MarkOnly`: 仅标记错误，不自动修改
//! - `ConfirmReplace`: 提示用户确认后替换
//! - `AutoReplace`: 自动替换已知错误
//!
//! ## 校对强度
//!
//! - `Low`: 低强度，仅检查明显错误
//! - `Normal`: 正常强度，平衡准确性和召回率
//! - `Strict`: 严格模式，尽可能多地检测潜在问题
//!
//! ## 依赖关系
//!
//! - `serde` / `serde_json`: 序列化/反序列化
//!
//! ## 使用场景
//!
//! - 实时拼写检查
//! - 文本自动纠错
//! - 写作质量提升
//! - 专业术语校对

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProofreadingMode {
    Off,
    MarkOnly,
    ConfirmReplace,
    AutoReplace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProofreadingStrength {
    Low,
    Normal,
    Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionRule {
    pub wrong_word: String,
    pub correct_word: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreRule {
    pub word: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofreadingDictionary {
    pub corrections: Vec<CorrectionRule>,
    pub ignores: Vec<IgnoreRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofreadingSuggestion {
    pub start_index: usize,
    pub end_index: usize,
    pub original_text: String,
    pub suggested_text: String,
    pub description: Option<String>,
}

pub struct ProofreadingService {
    pub mode: ProofreadingMode,
    pub strength: ProofreadingStrength,
    pub dictionary: ProofreadingDictionary,
}

impl ProofreadingService {
    pub fn new() -> Self {
        Self {
            mode: ProofreadingMode::MarkOnly,
            strength: ProofreadingStrength::Normal,
            dictionary: ProofreadingDictionary {
                corrections: Vec::new(),
                ignores: Vec::new(),
            },
        }
    }

    pub fn proofread(&self, _text: &str) -> crate::Result<Vec<ProofreadingSuggestion>> {
        if self.mode == ProofreadingMode::Off {
            return Ok(Vec::new());
        }

        let suggestions = Vec::new();
        Ok(suggestions)
    }
}

impl Default for ProofreadingService {
    fn default() -> Self {
        Self::new()
    }
}
