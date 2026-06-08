use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::PathBuf;
use writer_core::writing_stats::{store::StatsStore, EventSource, Platform, WritingInputEvent};

fn criterion_benchmark(c: &mut Criterion) {
    let mut events = Vec::new();
    for i in 0..1000 {
        events.push(WritingInputEvent {
            event_id: i.to_string(),
            timestamp_ms: 1717800000000 + (i * 1000), // Some arbitrary time
            device_id: format!("device_{}", i % 5),
            platform: Platform::Linux,
            project_id: "project_1".to_string(),
            volume_id: "volume_1".to_string(),
            chapter_id: "chapter_1".to_string(),
            session_id: "session_1".to_string(),
            source: EventSource::HumanTyped,
            inserted_chars: 10,
            deleted_chars: 0,
            pasted_chars: 0,
            ai_inserted_chars: 0,
            net_delta_chars: 10,
        });
    }

    let store = StatsStore::new(&PathBuf::from("/tmp/dummy_stats"));

    c.bench_function("aggregate_events", |b| {
        b.iter(|| store.aggregate_events(black_box(&events)).unwrap())
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
