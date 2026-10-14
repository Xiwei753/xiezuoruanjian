#![allow(clippy::unwrap_used)]
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use writer_core::writing_stats::{store::StatsStore, EventSource, Platform, WritingInputEvent};

fn criterion_benchmark(c: &mut Criterion) {
    let mut events = Vec::new();
    for i in 0..1000 {
        events.push(WritingInputEvent {
            event_id: i.to_string(),
            timestamp_ms: 1717800000000 + (i * 1000),
            device_id: format!("device_{}", i % 5),
            platform: Platform::Desktop,
            device_class: "desktop".to_string(),
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
            duration_seconds: 0,
            local_date: "2024-06-08".to_string(),
        });
    }

    let tmp = std::env::temp_dir().join(format!(
        "bench_stats_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = StatsStore::new(&tmp);

    c.bench_function("record_event", |b| {
        b.iter(|| {
            for event in black_box(&events) {
                store.record_event(event.clone()).unwrap();
            }
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
