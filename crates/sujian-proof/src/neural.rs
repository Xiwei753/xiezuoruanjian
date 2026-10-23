// SPDX-License-Identifier: GPL-3.0-only
//! Offline MacBERT4CSC character prediction.
//!
//! The ONNX graph is an equality-preserving masked-language-model head: for
//! each source token it produces a distribution over the vocabulary. This
//! module only exposes the original character and the strongest single-Han
//! alternative. It deliberately does not decide whether that alternative is
//! an error; calibration and author-owned protection live in `postprocess`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

use crate::postprocess::Calibrator;
use crate::tokenizer::{BertTokenizer, EncodedSentence, EncodedToken, TokenOrigin, TokenWindow};
use crate::CorrectionError;

/// One equal-length character proposal in original UTF-8 coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct PositionPrediction {
    pub start_byte: usize,
    pub end_byte: usize,
    pub original: char,
    pub candidate: char,
    pub original_log_probability: f32,
    pub candidate_log_probability: f32,
}

/// Raw model runner used by the correction pipeline.
///
/// Implementations may be the bundled ONNX model or a deterministic test
/// runner. The pipeline never inserts characters and never modifies the
/// caller's text.
pub trait NeuralPredictor: Send + Sync {
    fn predict(&self, text: &str) -> Result<Vec<PositionPrediction>, CorrectionError>;

    fn calibrator(&self) -> &Calibrator;
}

/// The single process-shared ONNX model. Weights are immutable; the ORT
/// session is serialized because `Session::run` requires exclusive access.
pub struct ProofreadingModel {
    session: Mutex<Session>,
    tokenizer: BertTokenizer,
    han_by_id: std::collections::HashMap<i64, char>,
    calibrator: Calibrator,
    has_token_type_ids: bool,
    model_path: PathBuf,
}

impl ProofreadingModel {
    /// Open a resource directory containing `model.onnx`, `vocab.txt`, and
    /// `calibrator.json`. The model is loaded from local disk only; there is
    /// no download path in this crate.
    pub fn open(bundle_dir: &Path) -> Result<Self, CorrectionError> {
        let model_path = bundle_dir.join("model.onnx");
        if !model_path.is_file() {
            return Err(CorrectionError::Model(format!(
                "proofreading model is missing {}",
                model_path.display()
            )));
        }
        ensure_runtime(bundle_dir)?;
        let tokenizer = BertTokenizer::open(&bundle_dir.join("vocab.txt"))?;
        let calibrator_path = bundle_dir.join("calibrator.json");
        let calibrator = if calibrator_path.is_file() {
            Calibrator::open(&calibrator_path)?
        } else {
            Calibrator::from_json(include_str!("../assets/correction-data/calibrator.json"))?
        };
        let session = Session::builder()
            .map_err(|error| CorrectionError::Model(error.to_string()))?
            .with_optimization_level(GraphOptimizationLevel::Level1)
            .map_err(|error| CorrectionError::Model(error.to_string()))?
            .commit_from_file(&model_path)
            .map_err(|error| {
                CorrectionError::Model(format!(
                    "failed to load ONNX model {}: {error}",
                    model_path.display()
                ))
            })?;
        let has_token_type_ids = session
            .inputs
            .iter()
            .any(|input| input.name == "token_type_ids");
        let han_by_id = tokenizer
            .han_ids()
            .iter()
            .map(|(id, ch)| (*id, *ch))
            .collect();
        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            han_by_id,
            calibrator,
            has_token_type_ids,
            model_path,
        })
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    fn predict_window(
        &self,
        session: &mut Session,
        tokens: &[EncodedToken],
        window: &TokenWindow,
    ) -> Result<Vec<(usize, PositionPrediction)>, CorrectionError> {
        let len = window.input_ids.len();
        let ids = window.input_ids.clone();
        let mask = window.attention_mask.clone();
        let types = window.token_type_ids.clone();
        let outputs = if self.has_token_type_ids {
            session
                .run(ort::inputs![
                    "input_ids" => model_tensor([1usize, len], ids)?,
                    "attention_mask" => model_tensor([1usize, len], mask)?,
                    "token_type_ids" => model_tensor([1usize, len], types)?,
                ])
                .map_err(|error| CorrectionError::Model(error.to_string()))?
        } else {
            session
                .run(ort::inputs![
                    "input_ids" => model_tensor([1usize, len], ids)?,
                    "attention_mask" => model_tensor([1usize, len], mask)?,
                ])
                .map_err(|error| CorrectionError::Model(error.to_string()))?
        };
        let output = outputs
            .iter()
            .next()
            .map(|(_, value)| value)
            .ok_or_else(|| CorrectionError::Model("model returned no outputs".to_string()))?;
        let (shape, logits) = output
            .try_extract_tensor::<f32>()
            .map_err(|error| CorrectionError::Model(error.to_string()))?;
        let dims = shape.as_ref();
        let (sequence, vocabulary) = match dims {
            [1, sequence, vocabulary] => (*sequence as usize, *vocabulary as usize),
            [sequence, vocabulary] => (*sequence as usize, *vocabulary as usize),
            _ => {
                return Err(CorrectionError::Model(format!(
                    "unexpected model output shape {dims:?}"
                )))
            }
        };
        if sequence != len {
            return Err(CorrectionError::Model(format!(
                "model returned {sequence} positions for {len} inputs"
            )));
        }
        if logits.len() < sequence.saturating_mul(vocabulary) {
            return Err(CorrectionError::Model(
                "model logits are shorter than the declared shape".to_string(),
            ));
        }

        let mut predictions = Vec::new();
        for (position, source) in window.source_indices.iter().enumerate() {
            let Some(source_index) = source else {
                continue;
            };
            let token = &tokens[*source_index];
            if token.origin != TokenOrigin::Han {
                continue;
            }
            let Some(original) = token.original else {
                continue;
            };
            let row = &logits[position * vocabulary..(position + 1) * vocabulary];
            let Some((log_sum_exp, original_logit)) = log_sum_exp_and(row, token.id) else {
                continue;
            };
            let Some((_candidate_id, candidate, candidate_logit)) =
                best_han_prediction(row, original, &self.han_by_id)
            else {
                continue;
            };
            predictions.push((
                window.edge_distance[position],
                PositionPrediction {
                    start_byte: token.start_byte,
                    end_byte: token.end_byte,
                    original,
                    candidate,
                    original_log_probability: (original_logit - log_sum_exp) as f32,
                    candidate_log_probability: (candidate_logit - log_sum_exp) as f32,
                },
            ));
        }
        Ok(predictions)
    }

    fn predict_encoded_window(
        &self,
        session: &mut Session,
        encoded: &EncodedSentence,
        window: &TokenWindow,
    ) -> Result<Vec<(usize, PositionPrediction)>, CorrectionError> {
        self.predict_window(session, &encoded.tokens, window)
    }
}

impl NeuralPredictor for ProofreadingModel {
    fn predict(&self, text: &str) -> Result<Vec<PositionPrediction>, CorrectionError> {
        let encoded = self.tokenizer.encode(text);
        let mut session = self
            .session
            .lock()
            .map_err(|_| CorrectionError::Model("model session lock poisoned".to_string()))?;
        let mut positions = BTreeMap::<usize, (usize, PositionPrediction)>::new();
        for window in encoded.windows() {
            for (edge, prediction) in
                self.predict_encoded_window(&mut session, &encoded, &window)?
            {
                positions
                    .entry(prediction.start_byte)
                    .and_modify(|(current_edge, current)| {
                        if edge > *current_edge {
                            *current_edge = edge;
                            *current = prediction.clone();
                        }
                    })
                    .or_insert((edge, prediction));
            }
        }
        Ok(positions
            .into_values()
            .map(|(_, prediction)| prediction)
            .collect())
    }

    fn calibrator(&self) -> &Calibrator {
        &self.calibrator
    }
}

fn ensure_runtime(bundle_dir: &Path) -> Result<(), CorrectionError> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    let explicit = std::env::var_os("SUJIAN_ONNXRUNTIME_LIB").map(PathBuf::from);
    let local = bundle_dir.join("libonnxruntime.so");
    let library = explicit.or_else(|| local.is_file().then_some(local));
    let Some(library) = library else {
        return Ok(());
    };
    let result = INIT.get_or_init(|| {
        ort::init_from(library.to_string_lossy().into_owned())
            .commit()
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    result
        .as_ref()
        .map(|_| ())
        .map_err(|error| CorrectionError::Model(format!("failed to load ONNX Runtime: {error}")))
}

fn model_tensor<const N: usize>(
    shape: [usize; N],
    data: Vec<i64>,
) -> Result<Tensor<i64>, CorrectionError> {
    Tensor::from_array((shape, data)).map_err(|error| CorrectionError::Model(error.to_string()))
}

fn log_sum_exp_and(values: &[f32], target: i64) -> Option<(f32, f32)> {
    let target = usize::try_from(target).ok()?;
    let target_logit = *values.get(target)?;
    let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !max.is_finite() {
        return None;
    }
    let sum = values.iter().map(|value| (*value - max).exp()).sum::<f32>();
    let log_sum_exp = max + sum.ln();
    log_sum_exp
        .is_finite()
        .then_some((log_sum_exp, target_logit))
}

fn best_han_prediction(
    logits: &[f32],
    original: char,
    han_by_id: &std::collections::HashMap<i64, char>,
) -> Option<(i64, char, f32)> {
    let (best_id, best_logit) = logits
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, logit)| logit.is_finite())
        .max_by(|left, right| left.1.total_cmp(&right.1))?;
    let best_id = i64::try_from(best_id).ok()?;
    let candidate = *han_by_id.get(&best_id)?;
    (candidate != original).then_some((best_id, candidate, best_logit))
}
