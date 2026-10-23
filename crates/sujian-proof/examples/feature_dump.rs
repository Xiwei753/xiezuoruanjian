use std::env;
use std::fs;
use std::sync::Arc;

use sujian_proof::{NeuralPredictor, ProofreadingModel};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let bundle = args.next().expect("bundle dir");
    let input = args.next().expect("input text");
    let output = args.next().expect("output csv");
    let prefix = args.next().unwrap_or_else(|| "normal".to_string());
    let model = Arc::new(ProofreadingModel::open(std::path::Path::new(&bundle))?);
    let text = fs::read_to_string(input)?;
    let mut rows = String::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.chars().count() > 500 {
            continue;
        }
        for prediction in model.predict(line)? {
            let margin = prediction.candidate_log_probability - prediction.original_log_probability;
            rows.push_str(&format!(
                "0,{:.9},{:.9},{:.9},{},{},{}-{}\n",
                prediction.original_log_probability,
                prediction.candidate_log_probability,
                margin,
                prediction.original,
                prediction.candidate,
                prefix,
                index
            ));
        }
    }
    fs::write(output, rows)?;
    Ok(())
}
