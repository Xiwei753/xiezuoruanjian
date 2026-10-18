//! 写作统计的**业务日历日**口径。
//!
//! 「今天纯输入多少字」这类产品语义必须按**用户当地的午夜**切日，
//! 不是按 UTC 午夜。仓库里 diagnostics 导出已经用 `chrono::Local`，
//! 所以日历日语义单侧收在这里，各平台不再各自拼日期字符串。
//!
//! ## 为什么不能直接拿 UTC timestamp 当「哪一天」
//!
//! `chrono::DateTime::from_timestamp_millis()` 返回 `DateTime<Utc>`，
//! 直接 `format("%Y-%m-%d")` 得到的是 UTC 日期。东八区本地 10 月 6 日 00:30
//! 输入的字会被算进 UTC 的 10 月 5 日，于是「今日纯输入」在本地凌晨 0 点
//! 提前清零、早上 8 点之后才开始正常累计；西半球则会在另一个时间段错一天。
//!
//! ## 两个口径的分工
//!
//! - **存储分区**：原始事件文件 `events.local/YYYY-MM-DD.events.jsonl`
//!   仍按 UTC 日期分文件（见 [`crate::writing_stats::StatsStore::timestamp_to_date`]）。
//!   改分文件规则要搬历史原始文件，收益不抵风险。
//! - **业务日历日**：[`local_date_at`] / [`local_date_from_offset`] /
//!   [`local_today_date`]，每日统计分桶和「今天」查询都走这个口径。

use crate::error::{Error, Result};
use crate::writing_stats::DateRange;
use chrono::{DateTime, Datelike, Months, NaiveDate};

/// 按给定 UTC 偏移（秒）把时间戳换算成 `YYYY-MM-DD` 业务日历日。
///
/// 抽成纯函数是为了能钉跨 UTC 日期的反例（UTC+8 的本地 00:30、
/// UTC-8 的本地 23:30），不依赖运行测试的机器处在哪个时区。
pub fn local_date_from_offset(timestamp_ms: i64, utc_offset_seconds: i32) -> Result<String> {
    let shifted = timestamp_ms + i64::from(utc_offset_seconds) * 1_000;
    let dt = DateTime::from_timestamp_millis(shifted)
        .ok_or_else(|| Error::Other("Invalid timestamp".to_string()))?;
    Ok(dt.format("%Y-%m-%d").to_string())
}

/// 时间戳所在时刻的**本机本地**日历日。
///
/// 用 `chrono::Local` 换算，夏令时切换当天也按真实本地偏移取值。
pub fn local_date_at(timestamp_ms: i64) -> Result<String> {
    let dt = DateTime::from_timestamp_millis(timestamp_ms)
        .ok_or_else(|| Error::Other("Invalid timestamp".to_string()))?;
    Ok(dt
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d")
        .to_string())
}

/// 本机当前时区的 UTC 偏移（秒）。
pub fn local_utc_offset_seconds() -> i32 {
    chrono::Local::now().offset().local_minus_utc()
}

/// 本机当前的业务日历日（`YYYY-MM-DD`）。
///
/// 「今天是几号」由 Core 决定，平台端不再自己拼本地日期字符串。
pub fn local_today_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 时间戳对应时刻的 UTC 业务日期（仅供存储分区与对照测试使用）。
///
/// 与 [`local_date_at`] 的区别：这个函数**不**换算到本地时区，
/// 直接取 UTC 日期。事件文件按它分目录。
pub fn utc_date_at(timestamp_ms: i64) -> Result<String> {
    let dt = DateTime::from_timestamp_millis(timestamp_ms)
        .ok_or_else(|| Error::Other("Invalid timestamp".to_string()))?;
    Ok(dt.format("%Y-%m-%d").to_string())
}

/// 把 `YYYY-MM-DD` 前后偏移 `days` 天，返回新的日期字符串。
pub fn shift_date(date: &str, days: i64) -> Result<String> {
    let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| Error::Other(format!("Invalid date {}: {}", date, e)))?;
    Ok((parsed + chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string())
}

/// 「本周」的起始业务日（周一，`YYYY-MM-DD`）。
///
/// 统计页面的「本周」按中文习惯以**周一**为一周开始，不用
/// `chrono::Weekday` 的周日口径；周起始只在 Core 算，端侧不再各自拼。
pub fn week_start_date(date: &str) -> Result<String> {
    let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| Error::Other(format!("Invalid date {}: {}", date, e)))?;
    let offset = i64::from(parsed.weekday().num_days_from_monday());
    Ok((parsed - chrono::Duration::days(offset))
        .format("%Y-%m-%d")
        .to_string())
}

/// 「本月」的起始业务日（当月 1 号，`YYYY-MM-DD`）。
pub fn month_start_date(date: &str) -> Result<String> {
    let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| Error::Other(format!("Invalid date {}: {}", date, e)))?;
    Ok(parsed
        .with_day(1)
        .ok_or_else(|| Error::Other(format!("Invalid date {}", date)))?
        .format("%Y-%m-%d")
        .to_string())
}

/// 解析 Core 业务日期（`YYYY-MM-DD`）。
pub fn parse_date(date: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| Error::Other(format!("Invalid date {}: {}", date, e)))
}

/// 把 `YYYY-MM-DD` 前后偏移 `months` 个日历月（月末钳制）。
///
/// 例：`2026-03-31` 往前 1 个月 → `2026-02-28`；`2026-08-31` 往前 6 个月
/// → `2026-02-28`。「近半年」按日历月而不是固定 180 天。
pub fn shift_months(date: &str, months: i64) -> Result<String> {
    let parsed = parse_date(date)?;
    let shifted = if months >= 0 {
        let months = u32::try_from(months)
            .map_err(|_| Error::Other(format!("Invalid month offset: {}", months)))?;
        parsed.checked_add_months(Months::new(months))
    } else {
        let months = u32::try_from(months.unsigned_abs())
            .map_err(|_| Error::Other(format!("Invalid month offset: {}", months)))?;
        parsed.checked_sub_months(Months::new(months))
    };
    let shifted =
        shifted.ok_or_else(|| Error::Other(format!("Invalid shifted month for {}", date)))?;
    Ok(shifted.format("%Y-%m-%d").to_string())
}

/// 闭区间 `[start_date, end_date]` 的自然日历天数（含两端）。
///
/// 「陪伴了 N 天」这类自然跨度用本函数，`start > end` 直接报错，
/// 不返回 0 冒充「没有跨度」。
pub fn day_span_inclusive(start_date: &str, end_date: &str) -> Result<u64> {
    let start = parse_date(start_date)?;
    let end = parse_date(end_date)?;
    if start > end {
        return Err(Error::Other(format!(
            "Invalid date span: {} .. {} (start must not be after end)",
            start_date, end_date
        )));
    }
    let days = (end - start).num_days() + 1;
    u64::try_from(days)
        .map_err(|_| Error::Other(format!("Invalid date span: {} .. {}", start_date, end_date)))
}

/// 统计页预设时段（#944）。
///
/// 字符串取值是端侧契约，固定为 `"last7Days"` / `"last30Days"` /
/// `"last6Months"` / `"lifetime"` / `"custom"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsPreset {
    Last7Days,
    Last30Days,
    Last6Months,
    Lifetime,
    Custom,
}

/// 时段卡片的固定顺序：近七天、近三十天、近半年、自选区间。
pub const PERIOD_CARD_PRESETS: [StatsPreset; 4] = [
    StatsPreset::Last7Days,
    StatsPreset::Last30Days,
    StatsPreset::Last6Months,
    StatsPreset::Custom,
];

impl StatsPreset {
    /// 解析端侧传入的 preset 字符串；未知字符串返回错误。
    pub fn parse(preset: &str) -> Result<Self> {
        match preset {
            "last7Days" => Ok(Self::Last7Days),
            "last30Days" => Ok(Self::Last30Days),
            "last6Months" => Ok(Self::Last6Months),
            "lifetime" => Ok(Self::Lifetime),
            "custom" => Ok(Self::Custom),
            other => Err(Error::Other(format!("Unknown stats preset: {}", other))),
        }
    }

    /// 稳定的 preset 字符串（跨端契约）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Last7Days => "last7Days",
            Self::Last30Days => "last30Days",
            Self::Last6Months => "last6Months",
            Self::Lifetime => "lifetime",
            Self::Custom => "custom",
        }
    }
}

/// 解析并校验 Explorer 查询参数（预设 + 可选自选起止日期）。
///
/// - preset 必须是五个固定字符串之一；
/// - 自选日期要么两端都不给，要么两端都是合法 `YYYY-MM-DD` 且
///   `start <= end`；**只给一端或格式错误一律报错**；
/// - `preset = "custom"` 必须带有效自选区间（选中时段的 `range` 不能是 null）。
///
/// 返回规范化后的闭区间（两端包含，保持用户选择的日期不被改成今天）。
/// 规则只在这里实现一份：FFI 边界校验和 Core 查询走同一个入口。
pub fn parse_explorer_query(
    preset: &str,
    custom_start: Option<&str>,
    custom_end: Option<&str>,
) -> Result<(StatsPreset, Option<DateRange>)> {
    let preset = StatsPreset::parse(preset)?;
    let start = custom_start.map(str::trim).filter(|s| !s.is_empty());
    let end = custom_end.map(str::trim).filter(|s| !s.is_empty());
    let range = match (start, end) {
        (None, None) => None,
        (Some(start), Some(end)) => {
            let parsed_start = parse_date(start)?;
            let parsed_end = parse_date(end)?;
            if parsed_start > parsed_end {
                return Err(Error::Other(format!(
                    "Invalid custom range: start {} is after end {}",
                    start, end
                )));
            }
            Some(DateRange {
                start_date: parsed_start.format("%Y-%m-%d").to_string(),
                end_date: parsed_end.format("%Y-%m-%d").to_string(),
            })
        }
        (Some(_), None) | (None, Some(_)) => {
            return Err(Error::Other(
                "Custom range requires both start and end date".to_string(),
            ))
        }
    };
    if preset == StatsPreset::Custom && range.is_none() {
        return Err(Error::Other(
            "custom preset requires both custom_start and custom_end".to_string(),
        ));
    }
    Ok((preset, range))
}

/// 解析某个时段实际使用的闭区间。
///
/// - `last7Days` / `last30Days`：含今天在内共 7 / 30 个自然日；
/// - `last6Months`：向前 6 个日历月到今天（月末钳制）；
/// - `lifetime`：首次写作日到今天；没有历史时为 今天..今天；
/// - `custom`：必须已有自选区间（未选择时由调用方走空卡分支）。
pub fn resolve_range(
    preset: StatsPreset,
    today: &str,
    first_writing_date: Option<&str>,
    custom_range: Option<&DateRange>,
) -> Result<DateRange> {
    match preset {
        StatsPreset::Last7Days => Ok(DateRange {
            start_date: shift_date(today, -6)?,
            end_date: today.to_string(),
        }),
        StatsPreset::Last30Days => Ok(DateRange {
            start_date: shift_date(today, -29)?,
            end_date: today.to_string(),
        }),
        StatsPreset::Last6Months => Ok(DateRange {
            start_date: shift_months(today, -6)?,
            end_date: today.to_string(),
        }),
        StatsPreset::Lifetime => Ok(lifetime_range(today, first_writing_date)),
        StatsPreset::Custom => custom_range.cloned().ok_or_else(|| {
            Error::Other("custom preset requires an explicit custom range".to_string())
        }),
    }
}

/// 生涯闭区间。
///
/// `first_writing_date` 正常不会晚于今天；若事件历史带着未来日期
/// （系统时钟被改快），退化成 今天..今天，不产出一个反向区间。
pub fn lifetime_range(today: &str, first_writing_date: Option<&str>) -> DateRange {
    let start = match first_writing_date {
        Some(first) if first <= today => first,
        _ => today,
    };
    DateRange {
        start_date: start.to_string(),
        end_date: today.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTC+8 的本地 10 月 6 日 00:30 == UTC 10 月 5 日 16:30。
    const TS_UTC8_LOCAL_0030: i64 = 1_791_217_800_000;
    /// UTC-8 的本地 10 月 5 日 23:30 == UTC 10 月 6 日 07:30。
    const TS_UTC8_LOCAL_2330: i64 = 1_791_271_800_000;

    #[test]
    fn test_local_date_utc_plus_8_0030_crosses_utc_date() {
        assert_eq!(utc_date_at(TS_UTC8_LOCAL_0030).unwrap(), "2026-10-05");
        assert_eq!(
            local_date_from_offset(TS_UTC8_LOCAL_0030, 8 * 3600).unwrap(),
            "2026-10-06"
        );
    }

    #[test]
    fn test_local_date_utc_minus_8_2330_crosses_utc_date() {
        assert_eq!(utc_date_at(TS_UTC8_LOCAL_2330).unwrap(), "2026-10-06");
        assert_eq!(
            local_date_from_offset(TS_UTC8_LOCAL_2330, -8 * 3600).unwrap(),
            "2026-10-05"
        );
    }

    #[test]
    fn test_local_date_matches_local_timezone() {
        // 本机口径必须和「用本机当前偏移换算」一致。
        let ts = chrono::Utc::now().timestamp_millis();
        assert_eq!(
            local_date_at(ts).unwrap(),
            local_date_from_offset(ts, local_utc_offset_seconds()).unwrap()
        );
    }

    #[test]
    fn test_shift_date_crosses_month_and_year() {
        assert_eq!(shift_date("2026-10-01", -1).unwrap(), "2026-09-30");
        assert_eq!(shift_date("2026-12-31", 1).unwrap(), "2027-01-01");
    }

    #[test]
    fn test_shift_date_rejects_garbage() {
        assert!(shift_date("not-a-date", 1).is_err());
    }

    #[test]
    fn test_week_start_date_monday_based() {
        // 2026-10-17 是周六，本周起始为周一 2026-10-12。
        assert_eq!(week_start_date("2026-10-17").unwrap(), "2026-10-12");
        // 周一自身就是起始日。
        assert_eq!(week_start_date("2026-10-12").unwrap(), "2026-10-12");
    }

    #[test]
    fn test_month_start_date() {
        assert_eq!(month_start_date("2026-10-17").unwrap(), "2026-10-01");
        assert_eq!(month_start_date("2026-01-31").unwrap(), "2026-01-01");
    }

    // ── #944 Explorer：预设与自选区间 ──

    #[test]
    fn test_parse_explorer_query_accepts_fixed_presets() {
        for (raw, expected) in [
            ("last7Days", StatsPreset::Last7Days),
            ("last30Days", StatsPreset::Last30Days),
            ("last6Months", StatsPreset::Last6Months),
            ("lifetime", StatsPreset::Lifetime),
        ] {
            let (preset, custom) = parse_explorer_query(raw, None, None).unwrap();
            assert_eq!(preset, expected);
            assert_eq!(preset.as_str(), raw);
            assert!(custom.is_none());
        }
    }

    #[test]
    fn test_parse_explorer_query_rejects_unknown_preset() {
        assert!(parse_explorer_query("lastWeek", None, None).is_err());
        assert!(parse_explorer_query("", None, None).is_err());
    }

    #[test]
    fn test_parse_explorer_query_custom_requires_both_ends() {
        let range = parse_explorer_query("custom", Some("2026-05-01"), Some("2026-05-31"))
            .unwrap()
            .1
            .unwrap();
        assert_eq!(range.start_date, "2026-05-01");
        assert_eq!(range.end_date, "2026-05-31");

        // custom 未选日期 / 只给一端 / 格式错 / 起止颠倒：一律报错。
        assert!(parse_explorer_query("custom", None, None).is_err());
        assert!(parse_explorer_query("custom", Some("2026-05-01"), None).is_err());
        assert!(parse_explorer_query("custom", None, Some("2026-05-31")).is_err());
        assert!(parse_explorer_query("custom", Some("not-a-date"), Some("2026-05-31")).is_err());
        assert!(parse_explorer_query("custom", Some("2026-05-31"), Some("2026-05-01")).is_err());
    }

    #[test]
    fn test_parse_explorer_query_custom_dates_optional_for_other_presets() {
        // 其他预设下自选日期是可选的：给一半照样报错，两端都给就带着走。
        assert!(parse_explorer_query("last7Days", Some("2026-05-01"), None).is_err());
        let (preset, custom) =
            parse_explorer_query("last7Days", Some("2026-05-01"), Some("2026-05-02")).unwrap();
        assert_eq!(preset, StatsPreset::Last7Days);
        assert_eq!(custom.unwrap().start_date, "2026-05-01");
    }

    #[test]
    fn test_parse_explorer_query_normalizes_non_padded_dates() {
        let range = parse_explorer_query("custom", Some("2026-5-1"), Some("2026-5-9"))
            .unwrap()
            .1
            .unwrap();
        assert_eq!(range.start_date, "2026-05-01");
        assert_eq!(range.end_date, "2026-05-09");
    }

    #[test]
    fn test_resolve_range_presets_include_today() {
        let today = "2026-10-18";
        let last7 = resolve_range(StatsPreset::Last7Days, today, None, None).unwrap();
        assert_eq!(last7.start_date, "2026-10-12");
        assert_eq!(last7.end_date, "2026-10-18");
        assert_eq!(
            day_span_inclusive(&last7.start_date, &last7.end_date).unwrap(),
            7
        );

        let last30 = resolve_range(StatsPreset::Last30Days, today, None, None).unwrap();
        assert_eq!(last30.start_date, "2026-09-19");
        assert_eq!(
            day_span_inclusive(&last30.start_date, &last30.end_date).unwrap(),
            30
        );

        let last6 = resolve_range(StatsPreset::Last6Months, today, None, None).unwrap();
        assert_eq!(last6.start_date, "2026-04-18");
        assert_eq!(last6.end_date, "2026-10-18");
    }

    #[test]
    fn test_last6_months_clamps_month_end() {
        // 8-31 往前 6 个日历月 = 2-28（2026 不是闰年），不是 3-03。
        let range = resolve_range(StatsPreset::Last6Months, "2026-08-31", None, None).unwrap();
        assert_eq!(range.start_date, "2026-02-28");
    }

    #[test]
    fn test_shift_months_clamps_and_crosses_year() {
        assert_eq!(shift_months("2026-03-31", -1).unwrap(), "2026-02-28");
        assert_eq!(shift_months("2026-01-31", -1).unwrap(), "2025-12-31");
        assert_eq!(shift_months("2026-11-30", 1).unwrap(), "2026-12-30");
        assert_eq!(shift_months("2026-12-31", 1).unwrap(), "2027-01-31");
        assert!(shift_months("2026-02-30", 1).is_err());
    }

    #[test]
    fn test_lifetime_range_uses_first_writing_date() {
        let range = lifetime_range("2026-10-18", Some("2025-12-24"));
        assert_eq!(range.start_date, "2025-12-24");
        assert_eq!(range.end_date, "2026-10-18");

        // 没有任何人工输入：退化成 今天..今天。
        let empty = lifetime_range("2026-10-18", None);
        assert_eq!(empty.start_date, "2026-10-18");
        assert_eq!(empty.end_date, "2026-10-18");

        // 未来日期（时钟漂移）不产出反向区间。
        let skewed = lifetime_range("2026-10-18", Some("2027-01-01"));
        assert_eq!(skewed.start_date, "2026-10-18");
    }

    #[test]
    fn test_day_span_inclusive_counts_both_ends() {
        assert_eq!(day_span_inclusive("2026-10-18", "2026-10-18").unwrap(), 1);
        assert_eq!(day_span_inclusive("2026-10-12", "2026-10-18").unwrap(), 7);
        assert_eq!(day_span_inclusive("2025-10-18", "2026-10-18").unwrap(), 366);
        assert!(day_span_inclusive("2026-10-18", "2026-10-17").is_err());
        assert!(day_span_inclusive("bad", "2026-10-17").is_err());
    }
}
