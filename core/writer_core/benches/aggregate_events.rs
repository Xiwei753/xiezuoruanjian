#![allow(clippy::unwrap_used)]
//! 写作统计事件写入基准。
//!
//! 每轮 iteration 在 setup 中新建独立临时目录、独立 `StatsStore` 和一段全新的
//! `event_id`，计时闭包只做写入。旧实现复用同一个 store 和同一批
//! `event_id=0..999`，第二轮起全部命中 `record_events` 的 ID 幂等去重，
//! 实际测的是判重而不是落盘。重复 ID 的去重基线单独放在
//! `record_events_duplicate_ids` 里。

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::{tempdir, TempDir};
use writer_core::writing_stats::{store::StatsStore, EventSource, Platform, WritingInputEvent};

/// 每轮写入的事件数，沿用旧基准的批量，便于横向比较。
const EVENTS_PER_ITERATION: usize = 1000;

/// 给每轮 setup 分配独立的事件 ID 段，保证计时闭包里的 ID 从未在该 store 出现。
static BATCH_SEQ: AtomicU64 = AtomicU64::new(0);

struct StatsBatch {
    dir: TempDir,
    store: StatsStore,
    events: Vec<WritingInputEvent>,
}

fn now_ms() -> i64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    i64::try_from(elapsed).unwrap()
}

fn new_event(batch: u64, index: usize, occurred_at_ms: i64) -> WritingInputEvent {
    WritingInputEvent::with_identity(
        format!("bench-{batch}-{index}"),
        occurred_at_ms + i64::try_from(index).unwrap(),
        "bench-device",
        Platform::Desktop,
        "desktop",
        "project-1",
        "volume-1",
        "chapter-1",
        EventSource::HumanTyped,
        10,
        0,
        0,
        0,
        0,
        "session-1",
    )
}

/// setup（不计时）：独立临时目录 + 独立 `StatsStore` + 新的事件 ID 段。
///
/// 事件时间取本次基准进程启动时采集的“现在”，保证落在当前 UTC 日：
/// `record_events` 写完后会顺手归档已关闭日期，若用固定历史日期，每轮计时
/// 都会混入一次 zstd 归档，测到的就不是纯写入路径。
fn fresh_batch(occurred_at_ms: i64) -> StatsBatch {
    let batch = BATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = tempdir().unwrap();
    let store = StatsStore::new(dir.path());
    // 设备身份解析会创建 settings/device 文件，放进 setup，不计入写入耗时。
    store.resolve_origin_device_id().unwrap();
    let events = (0..EVENTS_PER_ITERATION)
        .map(|index| new_event(batch, index, occurred_at_ms))
        .collect();
    StatsBatch { dir, store, events }
}

/// 单事件 API：每轮 1000 次 `record_event`，测逐事件追加的完整路径。
fn bench_record_event(c: &mut Criterion) {
    let occurred_at_ms = now_ms();
    c.bench_function("record_event", |b| {
        b.iter_batched(
            || fresh_batch(occurred_at_ms),
            |batch| {
                let StatsBatch { dir, store, events } = batch;
                for event in events {
                    store.record_event(event).unwrap();
                }
                // 目录必须活到计时结束之后：返回给 Criterion，在计时区外 drop。
                dir
            },
            BatchSize::SmallInput,
        );
    });
}

/// 批量 API：每轮一次 `record_events` 提交 1000 条全新事件。
fn bench_record_events(c: &mut Criterion) {
    let occurred_at_ms = now_ms();
    c.bench_function("record_events", |b| {
        b.iter_batched(
            || fresh_batch(occurred_at_ms),
            |batch| {
                let StatsBatch { dir, store, events } = batch;
                let written = store.record_events(events).unwrap();
                assert_eq!(written, EVENTS_PER_ITERATION);
                black_box(written);
                dir
            },
            BatchSize::SmallInput,
        );
    });
}

/// 判重基线：setup 先把 1000 条事件落盘，计时只做同一批 ID 的重复提交。
///
/// 与上面的写入基准分开命名，避免把“幂等判重”混进“真实写入”的结论。
fn bench_record_events_duplicate_ids(c: &mut Criterion) {
    let occurred_at_ms = now_ms();
    c.bench_function("record_events_duplicate_ids", |b| {
        b.iter_batched(
            || {
                let batch = fresh_batch(occurred_at_ms);
                let written = batch.store.record_events(batch.events.clone()).unwrap();
                assert_eq!(written, EVENTS_PER_ITERATION);
                batch
            },
            |batch| {
                let StatsBatch { dir, store, events } = batch;
                let written = store.record_events(events).unwrap();
                assert_eq!(written, 0, "同一 store 重复提交相同 event_id 必须全部判重");
                black_box(written);
                dir
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_record_event,
    bench_record_events,
    bench_record_events_duplicate_ids
);
criterion_main!(benches);
