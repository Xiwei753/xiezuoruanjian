use std::path::Path;
use std::sync::Arc;
use sujian_proof::{NeuralPredictor, CorrectionConfig, CorrectionEngine, Lexicon, ProofreadingModel, RuleSet};
fn main() {
    let model = Arc::new(ProofreadingModel::open(Path::new("/tmp/macbert4csc-v2")).unwrap());
    for text in ["不再看宿禾。","其中考试开始了。","他画了一幅画。","这因该算好了。","他按装了灯。","他种了高梁。","他头资了很多钱。"] {
        let preds=model.predict(text).unwrap();
        println!("TEXT {text}");
        for p in preds { println!("  {} {} -> {} cand={:.6} orig={:.6} margin={:.6}",p.start_byte,p.original,p.candidate,p.candidate_log_probability,p.original_log_probability,p.candidate_log_probability-p.original_log_probability); }
    }
    let engine = CorrectionEngine::new_shared(model, Arc::new(Lexicon::default()), RuleSet::empty(), CorrectionConfig::default()).unwrap();
    for text in ["不再看宿禾。","其中考试开始了。","他画了一幅画。","这因该算好了。","他按装了灯。","他种了高梁。","他头资了很多钱。"] {
        println!("SCAN {text}");
        for d in engine.scan(text).unwrap() { println!("  {} -> {} conf={:?}",d.original,d.replacement,d.confidence); }
    }
}
