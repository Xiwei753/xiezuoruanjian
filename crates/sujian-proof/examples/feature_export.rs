use std::env;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;
use sujian_proof::{NeuralPredictor, ProofreadingModel};

#[derive(Deserialize)]
struct Row {
    original_text: String,
    correct_text: String,
    #[serde(default)]
    wrong_ids: Vec<usize>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let bundle = args.next().expect("bundle");
    let dataset = args.next().expect("dataset");
    let extra = args.next().expect("extra text");
    let output = args.next().expect("output");
    let limit: usize = args.next().map_or(0, |value| value.parse().unwrap());
    let model = Arc::new(ProofreadingModel::open(Path::new(&bundle))?);
    let rows: Vec<Row> = serde_json::from_str(&fs::read_to_string(dataset)?)?;
    let mut csv = String::from(
        "label,original_logprob,candidate_logprob,margin,original_char,candidate_char,sample_id\n",
    );
    let mut positive = 0usize;
    let mut negative = 0usize;
    let mut misses = 0usize;
    let mut used = 0usize;
    for row in &rows {
        if limit > 0 && used >= limit {
            break;
        }
        if row.original_text.is_empty() || row.original_text.chars().count() > 500 {
            continue;
        }
        let predictions = model.predict(&row.original_text)?;
        used += 1;
        let annotated: std::collections::HashSet<usize> =
            row.wrong_ids.iter().copied().collect();
        let corrected: Vec<char> = row.correct_text.chars().collect();
        let original_chars: Vec<char> = row.original_text.chars().collect();
        let mut predicted_at = std::collections::HashSet::new();
        for prediction in &predictions {
            let Some(char_index) = row
                .original_text
                .get(..prediction.start_byte)
                .map(|prefix| prefix.chars().count())
            else {
                continue;
            };
            if annotated.contains(&char_index) {
                predicted_at.insert(char_index);
                if corrected.get(char_index) == Some(&prediction.candidate) {
                    push_row(&mut csv, 1, prediction, &format!("csc-{used}"));
                    positive += 1;
                } else {
                    misses += 1;
                }
                continue;
            }
            push_row(&mut csv, 0, prediction, &format!("csc-{used}"));
            negative += 1;
        }
        for char_index in annotated {
            if char_index < original_chars.len() && !predicted_at.contains(&char_index) {
                misses += 1;
            }
        }
        if row.correct_text != row.original_text
            && row.correct_text.chars().count() == original_chars.len()
        {
            for prediction in model.predict(&row.correct_text)? {
                push_row(&mut csv, 0, &prediction, &format!("csc-{used}"));
                negative += 1;
            }
        }
    }
    if Path::new(&extra).is_file() {
        for (line_index, line) in fs::read_to_string(extra)?.lines().enumerate() {
            let text = line.trim();
            if text.is_empty() || text.chars().count() > 500 {
                continue;
            }
            for prediction in model.predict(text)? {
                push_row(
                    &mut csv,
                    0,
                    &prediction,
                    &format!("constructed-{line_index}"),
                );
                negative += 1;
            }
        }
    }
    fs::write(output, csv)?;
    eprintln!(
        "samples={used} positive={positive} negative={negative} misses={misses}"
    );
    Ok(())
}

fn push_row(csv: &mut String, label: u8, prediction: &sujian_proof::PositionPrediction, id: &str) {
    let margin = prediction.candidate_log_probability - prediction.original_log_probability;
    csv.push_str(&format!(
        "{label},{:.9},{:.9},{:.9},{},{},{id}\n",
        prediction.original_log_probability,
        prediction.candidate_log_probability,
        margin,
        prediction.original,
        prediction.candidate
    ));
}
