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
use chrono::DateTime;

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
    use chrono::Offset;
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
}
