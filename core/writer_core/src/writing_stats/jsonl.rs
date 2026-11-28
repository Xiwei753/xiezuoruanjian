//! 统计事件 JSONL 的共用底层。
//!
//! 写作输入事件、计时切片、对象贡献事件与 AI 用量事件共用
//! `stats/events/<device_id>/*.jsonl` 分区布局。本模块只负责底层文件事实：
//!
//! - 严格按行解析：IO 与 JSON 错误携带文件路径与 1 基行号，不做 best-effort 跳过；
//! - 稳定 ID 扫描：旧格式允许空 ID 的真实历史记录照常解析，但不进入去重集合；
//! - 追加：完整记录缺换行时补分隔符，追加后 `sync_all`，新建分区同步父目录；
//! - 崩溃残行：只有尚存 pending 的确定事件序列化行能证明尾部残片属于本次追加时，
//!   才截去该残片；中间损坏或无法匹配的尾部保持原文件并返回错误。
//!
//! 业务层各自保留自己的结构与语义，本模块不判断事件归属、不补默认值。

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

/// 一条已解析的 JSONL 记录。
pub(crate) struct JsonlRecord<T> {
    /// 1 基物理行号（空行也占一行）。
    pub line: usize,
    pub value: T,
}

/// 稳定 ID 扫描结果。
pub(crate) struct JsonlIdScan {
    /// 非空稳定 ID 集合。
    pub ids: HashSet<String>,
    /// 没有稳定 ID 的物理行号；旧格式的真实历史记录，调用方按各自语义处理。
    pub lines_without_id: Vec<usize>,
}

fn open_for_read(path: &Path) -> Result<Option<File>> {
    match File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Other(format!(
            "failed to open stats jsonl {}: {error}",
            path.display()
        ))),
    }
}

fn invalid_json(path: &Path, line: usize, error: impl std::fmt::Display) -> Error {
    Error::Other(format!(
        "invalid JSON in stats jsonl {} line {line}: {error}",
        path.display()
    ))
}

/// 严格读取一个 JSONL 文件；文件不存在返回空集合，其它读取错误原样返回。
pub(crate) fn read_jsonl<T: DeserializeOwned>(path: &Path) -> Result<Vec<JsonlRecord<T>>> {
    let Some(file) = open_for_read(path)? else {
        return Ok(Vec::new());
    };
    let mut records = Vec::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = index + 1;
        let line = line.map_err(|error| {
            Error::Other(format!(
                "failed to read stats jsonl {} line {line_number}: {error}",
                path.display()
            ))
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value =
            serde_json::from_str(trimmed).map_err(|error| invalid_json(path, line_number, error))?;
        records.push(JsonlRecord {
            line: line_number,
            value,
        });
    }
    Ok(records)
}

/// 扫描文件中的稳定 ID。
///
/// 每行必须能按业务结构解析；空 ID 的记录会记录行号而不是被当成坏数据。
pub(crate) fn read_jsonl_ids<T, F>(path: &Path, id_of: F) -> Result<JsonlIdScan>
where
    T: DeserializeOwned,
    F: for<'a> Fn(&'a T) -> &'a str,
{
    let mut ids = HashSet::new();
    let mut lines_without_id = Vec::new();
    for record in read_jsonl::<T>(path)? {
        let id = id_of(&record.value);
        if id.is_empty() {
            lines_without_id.push(record.line);
        } else {
            ids.insert(id.to_string());
        }
    }
    Ok(JsonlIdScan {
        ids,
        lines_without_id,
    })
}

/// 同步目录项；仅 Unix 系平台支持打开目录，其它平台由文件 `sync_all` 覆盖。
pub(crate) fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// 读取文件末尾未以换行结束的片段；文件为空或正好以换行结束返回空串。
fn read_trailing_fragment(file: &mut File, len: u64) -> Result<String> {
    if len == 0 {
        return Ok(String::new());
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        return Ok(String::new());
    }
    let mut start = len;
    let mut collected: Vec<u8> = Vec::new();
    while start > 0 {
        let chunk_len = start.min(4096);
        start -= chunk_len;
        let mut buf = vec![0u8; chunk_len as usize];
        file.seek(SeekFrom::Start(start))?;
        file.read_exact(&mut buf)?;
        if let Some(index) = buf.iter().rposition(|byte| *byte == b'\n') {
            collected.splice(0..0, buf[index + 1..].iter().copied());
            break;
        }
        collected.splice(0..0, buf.iter().copied());
    }
    String::from_utf8(collected)
        .map_err(|error| Error::Other(format!("stats jsonl is not valid UTF-8: {error}")))
}

/// 追加一批已序列化的 JSONL 行，每行补一个换行。
///
/// 末尾若是完整记录但缺换行，只补分隔符，不覆盖任何已有字节；末尾是无法
/// 解析成 JSON 的残片时返回错误，不把残片固化成一条坏记录。
pub(crate) fn append_jsonl_lines(path: &Path, lines: &[String]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let existed = path.exists();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    let len = file.metadata()?.len();
    let fragment = read_trailing_fragment(&mut file, len)?;
    if !fragment.is_empty() && serde_json::from_str::<serde_json::Value>(&fragment).is_err() {
        return Err(Error::Other(format!(
            "refuse to append stats jsonl {}: trailing fragment is not a complete JSON record",
            path.display()
        )));
    }
    let mut buffer = String::new();
    if !fragment.is_empty() {
        buffer.push('\n');
    }
    for line in lines {
        buffer.push_str(line);
        buffer.push('\n');
    }
    file.write_all(buffer.as_bytes())?;
    file.sync_all()?;
    drop(file);
    if !existed {
        if let Some(parent) = path.parent() {
            sync_dir(parent)?;
        }
    }
    Ok(())
}

/// 依据仍存 pending 的确定事件修复崩溃留下的尾部残行。
///
/// - 完整行必须能按业务结构严格解析；中间损坏不修复，保留原文件并返回错误。
/// - 末尾是完整记录但缺换行：保留，由 [`append_jsonl_lines`] 补分隔符。
/// - 末尾是某个期望事件序列化行的真前缀：可证明属于本次追加，截去残片。
/// - 末尾无法被期望行证明：保留原文件并返回错误。
pub(crate) fn repair_tail_with_pending<T: DeserializeOwned>(
    path: &Path,
    expected_lines: &[String],
) -> Result<()> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read stats jsonl {}: {error}",
                path.display()
            )))
        }
    };
    if bytes.is_empty() {
        return Ok(());
    }
    let complete_end = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    for (index, line) in bytes[..complete_end].split(|byte| *byte == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(line).map_err(|error| {
            Error::Other(format!(
                "stats jsonl {} line {} is not valid UTF-8: {error}",
                path.display(),
                index + 1
            ))
        })?;
        serde_json::from_str::<T>(text.trim())
            .map_err(|error| invalid_json(path, index + 1, error))?;
    }
    let tail = &bytes[complete_end..];
    if tail.is_empty() {
        return Ok(());
    }
    let tail_text = std::str::from_utf8(tail).map_err(|error| {
        Error::Other(format!(
            "trailing fragment of {} is not valid UTF-8: {error}",
            path.display()
        ))
    })?;
    // 完整记录只是缺换行：保留事实，交给追加函数补分隔符。
    if serde_json::from_str::<T>(tail_text).is_ok() {
        return Ok(());
    }
    if expected_lines
        .iter()
        .any(|line| line.as_bytes().starts_with(tail))
    {
        let file = OpenOptions::new().write(true).open(path)?;
        file.set_len(complete_end as u64)?;
        file.sync_all()?;
        return Ok(());
    }
    Err(Error::Other(format!(
        "stats jsonl {} ends with an unprovable {}-byte fragment; pending events do not prove it belongs to the interrupted append",
        path.display(),
        tail.len()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sujian-stats-jsonl-{}-{}",
            name,
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join("events.jsonl")
    }

    #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
    struct TestEvent {
        event_id: String,
        value: u32,
    }

    #[test]
    fn strict_read_reports_line_number() {
        let path = temp_file("strict");
        fs::write(&path, "{\"event_id\":\"a\",\"value\":1}\nnot json\n").unwrap();
        let error = read_jsonl::<TestEvent>(&path).unwrap_err().to_string();
        assert!(error.contains("line 2"), "{error}");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn id_scan_keeps_empty_id_lines() {
        let path = temp_file("ids");
        fs::write(
            &path,
            "{\"event_id\":\"a\",\"value\":1}\n{\"event_id\":\"\",\"value\":2}\n",
        )
        .unwrap();
        let scan = read_jsonl_ids::<TestEvent, _>(&path, |event| event.event_id.as_str()).unwrap();
        assert!(scan.ids.contains("a"));
        assert_eq!(scan.lines_without_id, vec![2]);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn append_adds_separator_after_complete_record_without_newline() {
        let path = temp_file("separator");
        fs::write(&path, "{\"event_id\":\"a\",\"value\":1}").unwrap();
        append_jsonl_lines(&path, &["{\"event_id\":\"b\",\"value\":2}".to_string()]).unwrap();
        let records = read_jsonl::<TestEvent>(&path).unwrap();
        assert_eq!(records.len(), 2);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn append_refuses_unproven_fragment() {
        let path = temp_file("fragment");
        fs::write(&path, "{\"event_id\":\"a\",\"val").unwrap();
        assert!(append_jsonl_lines(&path, &["{\"event_id\":\"b\",\"value\":2}".to_string()]).is_err());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn repair_truncates_proven_prefix_only() {
        let path = temp_file("repair");
        let expected = "{\"event_id\":\"b\",\"value\":2}".to_string();
        fs::write(&path, "{\"event_id\":\"a\",\"value\":1}\n{\"event_i").unwrap();
        repair_tail_with_pending::<TestEvent>(&path, std::slice::from_ref(&expected)).unwrap();
        append_jsonl_lines(&path, &[expected]).unwrap();
        let records = read_jsonl::<TestEvent>(&path).unwrap();
        assert_eq!(records.len(), 2);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
