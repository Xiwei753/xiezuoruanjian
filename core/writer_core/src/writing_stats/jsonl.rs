//! 统计事件 JSONL 的共用底层。
//!
//! 写作输入事件、计时切片、对象贡献事件与 AI 用量事件共用
//! `stats/events/<device_id>/*.jsonl` 分区布局。本模块只负责底层文件事实：
//!
//! - 严格按行解析：IO 与 JSON 错误携带文件路径与 1 基行号，不做 best-effort 跳过；
//! - 稳定 ID 扫描：旧格式允许空 ID 的真实历史记录照常解析，但不进入去重集合；
//! - 追加：完整记录缺换行时补分隔符，追加后 `sync_all`，新建分区同步父目录；
//!   即使本次没有新行，也对已存在文件补一次持久化确认；
//! - 崩溃残行：只有调用方提供的确定事件序列化行能证明尾部残片属于本次追加时，
//!   才截去该残片；中间损坏或无法匹配的尾部保持原文件并返回错误。
//!
//! 业务层各自保留自己的结构与语义，本模块不判断事件归属、不补默认值。

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde::de::DeserializeOwned;

use crate::error::{Error, Result};

/// 一条已解析的 JSONL 记录。
#[derive(Debug)]
pub(crate) struct JsonlRecord<T> {
    /// 1 基物理行号（空行也占一行）。
    pub line: usize,
    pub value: T,
}

/// 稳定 ID 扫描结果。
#[derive(Debug, Default)]
pub(crate) struct JsonlIdScan {
    /// 非空稳定 ID 集合。
    pub ids: HashSet<String>,
    /// 没有稳定 ID 的物理行号；旧格式的真实历史记录，调用方按各自语义处理。
    pub lines_without_id: Vec<usize>,
    /// 非空 ID 对应原始行（去首尾空白）的稳定指纹，用于识别同一 ID 的
    /// 不同 payload；归档段等无法取原始行时可以为 0。
    pub fingerprints: HashMap<String, u64>,
}

/// 原始 JSONL 行的稳定指纹（FNV-1a 64）。
pub(crate) fn line_fingerprint(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
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

fn invalid_json_bytes(path: &Path, line: usize, error: impl std::fmt::Display) -> Error {
    Error::Other(format!(
        "invalid JSON in stats jsonl {} line {line} (raw bytes): {error}",
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

/// 扫描文件中的稳定 ID（严格流式读取，逐行丢弃已解析值）。
///
/// 每行必须能按业务结构解析；空 ID 的记录会记录行号而不是被当成坏数据。
pub(crate) fn read_jsonl_ids<T, F>(path: &Path, id_of: F) -> Result<JsonlIdScan>
where
    T: DeserializeOwned,
    F: for<'a> Fn(&'a T) -> &'a str,
{
    scan_ids_impl(path, id_of, None, false, false)
}

/// 扫描稳定 ID，但容忍文件末尾未写完的最后一行（不截断、不报错）。
///
/// 只用于“根据已有 ID 判重”的读路径：尾部残片没有换行，不可能是完整事件；
/// 真正的追加/恢复路径会用本批确定事件的序列化行匹配后再截断。这样合法
/// 中断的写入不会因为一次严格判重而永久堵住后续写入。
pub(crate) fn read_jsonl_ids_tolerant_tail<T, F>(
    path: &Path,
    id_of: F,
) -> Result<JsonlIdScan>
where
    T: DeserializeOwned,
    F: for<'a> Fn(&'a T) -> &'a str,
{
    scan_ids_impl(path, id_of, None, false, true)
}

/// 扫描稳定 ID，并允许用本批确定事件修复“未以换行结束”的尾部残片。
///
/// 残片只在原始字节上匹配本批某条事件序列化行的真前缀时才截断；已是完整
/// JSON 的尾部记录照常计入 ID，由追加函数补分隔符。语法中间损坏仍报错。
pub(crate) fn read_jsonl_ids_repairing_tail<T, F>(
    path: &Path,
    id_of: F,
    expected_lines: &[String],
) -> Result<JsonlIdScan>
where
    T: DeserializeOwned,
    F: for<'a> Fn(&'a T) -> &'a str,
{
    scan_ids_impl(path, id_of, Some(expected_lines), true, false)
}

/// 扫描稳定 ID 与调用方给出的业务事实指纹，容忍文件末尾未写完的尾行。
///
/// 同一 ID 重复出现时只接受相同业务指纹；不同业务事实直接报错，不让最后
/// 一条静默覆盖前面的真实记录。用于证明事件按 journal 原始事实判重。
pub(crate) fn read_jsonl_identities_tolerant_tail<T, F, I>(
    path: &Path,
    id_of: F,
    identity_of: I,
) -> Result<HashMap<String, u64>>
where
    T: DeserializeOwned,
    F: for<'a> Fn(&'a T) -> &'a str,
    I: Fn(&T) -> Result<u64>,
{
    let Some(mut file) = open_for_read(path)? else {
        return Ok(HashMap::new());
    };
    let mut identities = HashMap::new();
    let mut buffer = Vec::new();
    let mut line_number = 0usize;
    loop {
        buffer.clear();
        let read = file.read_until(b'\n', &mut buffer)?;
        if read == 0 {
            break;
        }
        line_number += 1;
        if buffer.last() != Some(&b'\n') {
            // 未完成尾行不可能是完整事件；追加路径会按确定事实修复。
            break;
        }
        buffer.pop();
        if buffer.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }
        let value: T = serde_json::from_slice(&buffer)
            .map_err(|error| invalid_json_bytes(path, line_number, error))?;
        let id = id_of(&value);
        if id.is_empty() {
            continue;
        }
        let identity = identity_of(&value)?;
        match identities.get(id) {
            Some(stored) if *stored == identity => {}
            Some(_) => {
                return Err(Error::Other(format!(
                    "stats jsonl {} line {} stores event {} with conflicting business facts; refusing to pick one",
                    path.display(),
                    line_number,
                    id
                )))
            }
            None => {
                identities.insert(id.to_string(), identity);
            }
        }
    }
    Ok(identities)
}

/// 只修复未以换行结束的尾部残片，不扫描完整文件。
///
/// 用于“同一批多个分区必须先全部完成尾部修复，再开始任何追加”的恢复路径。
pub(crate) fn repair_tail_fragment<T: DeserializeOwned>(
    path: &Path,
    expected_lines: &[String],
) -> Result<()> {
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to open stats jsonl {}: {error}",
                path.display()
            )))
        }
    };
    let len = file.metadata()?.len();
    let tail = read_trailing_fragment_bytes(&mut file, len)?;
    if tail.is_empty() {
        return Ok(());
    }
    if serde_json::from_slice::<T>(&tail).is_ok() {
        return Ok(());
    }
    if !expected_prefix_matches(&tail, expected_lines) {
        return Err(Error::Other(format!(
            "stats jsonl {} ends with an unprovable {}-byte fragment; pending events do not prove it belongs to the interrupted append",
            path.display(),
            tail.len()
        )));
    }
    truncate_tail(&mut file, len, tail.len())?;
    Ok(())
}

fn scan_ids_impl<T, F>(
    path: &Path,
    id_of: F,
    expected_lines: Option<&[String]>,
    repair_tail: bool,
    ignore_incomplete_tail: bool,
) -> Result<JsonlIdScan>
where
    T: DeserializeOwned,
    F: for<'a> Fn(&'a T) -> &'a str,
{
    let Some(file) = open_for_read(path)? else {
        return Ok(JsonlIdScan::default());
    };
    let mut expected_by_id: HashMap<String, &str> = HashMap::new();
    if let Some(lines) = expected_lines {
        for line in lines {
            if let Ok(value) = serde_json::from_str::<T>(line) {
                let id = id_of(&value);
                if !id.is_empty() {
                    expected_by_id.insert(id.to_string(), line.as_str());
                }
            }
        }
    }
    let mut reader = BufReader::new(file);
    let mut scan = JsonlIdScan::default();
    let mut buffer: Vec<u8> = Vec::new();
    let mut line_number = 0usize;
    let mut offset = 0u64;
    loop {
        buffer.clear();
        let read = reader.read_until(b'\n', &mut buffer).map_err(|error| {
            Error::Other(format!(
                "failed to read stats jsonl {} line {}: {error}",
                path.display(),
                line_number + 1
            ))
        })?;
        if read == 0 {
            break;
        }
        let line_start = offset;
        offset = offset.saturating_add(read as u64);
        line_number += 1;
        let complete = buffer.last() == Some(&b'\n');
        let raw: &[u8] = if complete {
            &buffer[..buffer.len() - 1]
        } else {
            &buffer[..]
        };
        if raw.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }
        match serde_json::from_slice::<T>(raw) {
            Ok(value) => {
                let id = id_of(&value);
                if id.is_empty() {
                    scan.lines_without_id.push(line_number);
                    continue;
                }
                if let Some(expected) = expected_by_id.get(id) {
                    let line = std::str::from_utf8(raw).map_err(|error| {
                        invalid_json_bytes(path, line_number, error)
                    })?;
                    if line.trim() != expected.trim() {
                        return Err(Error::Other(format!(
                            "stats jsonl {} line {} already stores event {} with a different payload; refusing to treat it as written",
                            path.display(),
                            line_number,
                            id
                        )));
                    }
                }
                let fingerprint = line_fingerprint(trim_ascii(raw));
                if let Some(stored) = scan.fingerprints.get(id) {
                    if *stored != fingerprint {
                        return Err(Error::Other(format!(
                            "stats jsonl {} line {} stores event {} with a different payload than an earlier line; refusing to pick one",
                            path.display(),
                            line_number,
                            id
                        )));
                    }
                    continue;
                }
                scan.fingerprints.insert(id.to_string(), fingerprint);
                scan.ids.insert(id.to_string());
            }
            Err(error) => {
                if !complete && ignore_incomplete_tail {
                    // 尾片没有换行，不可能是完整事件；判重先忽略它，真正的
                    // 追加路径会按确定事件序列化行证明后截断。
                    break;
                }
                if !complete && repair_tail {
                    let Some(expected) = expected_lines else {
                        return Err(invalid_json_bytes(path, line_number, error));
                    };
                    if expected_prefix_matches(raw, expected) {
                        // 尾片是本批某条确定事件的原始字节前缀：截断后由追加
                        // 函数重新写入完整记录；同步在截断时立即完成。
                        let mut file = OpenOptions::new().write(true).open(path)?;
                        file.set_len(line_start)?;
                        file.sync_all()?;
                        break;
                    }
                }
                return Err(invalid_json_bytes(path, line_number, error));
            }
        }
    }
    Ok(scan)
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while let Some((first, rest)) = bytes.split_first() {
        if !first.is_ascii_whitespace() {
            break;
        }
        bytes = rest;
    }
    while let Some((last, rest)) = bytes.split_last() {
        if !last.is_ascii_whitespace() {
            break;
        }
        bytes = rest;
    }
    bytes
}

fn expected_prefix_matches(tail: &[u8], expected_lines: &[String]) -> bool {
    expected_lines
        .iter()
        .any(|line| line.as_bytes().starts_with(tail))
}

fn truncate_tail(file: &mut File, len: u64, tail_len: usize) -> Result<()> {
    let complete_end = len.saturating_sub(tail_len as u64);
    file.set_len(complete_end)?;
    file.sync_all()?;
    Ok(())
}

/// 读取文件末尾未以换行结束的片段；文件为空或正好以换行结束返回空串。
fn read_trailing_fragment_bytes(file: &mut File, len: u64) -> Result<Vec<u8>> {
    if len == 0 {
        return Ok(Vec::new());
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        return Ok(Vec::new());
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
    Ok(collected)
}

/// 截掉文件末尾无法解析成完整记录的残片；完整 JSON（即使没有换行）保留。
///
/// 用于重试恢复：残片不是事件，journal 会按原始事实重写，不能因为它保存的
/// 派生 seq/hash 已变化而永久拒绝恢复。
pub(crate) fn truncate_incomplete_tail<T: DeserializeOwned>(path: &Path) -> Result<bool> {
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to open stats jsonl {}: {error}",
                path.display()
            )))
        }
    };
    let len = file.metadata()?.len();
    let tail = read_trailing_fragment_bytes(&mut file, len)?;
    if tail.is_empty() || serde_json::from_slice::<T>(&tail).is_ok() {
        return Ok(false);
    }
    truncate_tail(&mut file, len, tail.len())?;
    Ok(true)
}

/// 流式读取指定 ID 的完整记录，尾部未完成残片忽略；中间损坏仍报错。
///
/// 恢复路径只取本批真正需要的旧事件，不把整日文件读成 `Vec`。
pub(crate) fn read_jsonl_matching_ids<T, F>(
    path: &Path,
    wanted: &HashSet<String>,
    id_of: F,
) -> Result<Vec<JsonlRecord<T>>>
where
    T: DeserializeOwned,
    F: for<'a> Fn(&'a T) -> &'a str,
{
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let Some(file) = open_for_read(path)? else {
        return Ok(Vec::new());
    };
    let mut records = Vec::new();
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    let mut line_number = 0usize;
    loop {
        buffer.clear();
        let read = reader.read_until(b'\n', &mut buffer).map_err(|error| {
            Error::Other(format!(
                "failed to read stats jsonl {} line {}: {error}",
                path.display(),
                line_number + 1
            ))
        })?;
        if read == 0 {
            break;
        }
        line_number += 1;
        let complete = buffer.last() == Some(&b'\n');
        let raw: &[u8] = if complete {
            &buffer[..buffer.len() - 1]
        } else {
            &buffer[..]
        };
        if raw.iter().all(|byte| byte.is_ascii_whitespace()) {
            continue;
        }
        match serde_json::from_slice::<T>(raw) {
            Ok(value) if wanted.contains(id_of(&value)) => records.push(JsonlRecord {
                line: line_number,
                value,
            }),
            Ok(_) => {}
            Err(_) if !complete => break,
            Err(error) => return Err(invalid_json_bytes(path, line_number, error)),
        }
    }
    Ok(records)
}

/// 确认一个已有 JSONL 文件的内容已持久化。
///
/// 用于“ID 已在磁盘上所以跳过追加”的去重命中：读得到不代表上次 `write_all`
/// 后的 `sync_all` 成功，确认前不能清除 pending/journal 责任。
pub(crate) fn confirm_durable(path: &Path) -> Result<()> {
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::Other(format!(
                "stats jsonl {} is missing while confirming a previously written event",
                path.display()
            )))
        }
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to open stats jsonl {} for durability confirmation: {error}",
                path.display()
            )))
        }
    };
    let len = file.metadata()?.len();
    let fragment = read_trailing_fragment_bytes(&mut file, len)?;
    if !fragment.is_empty() {
        if serde_json::from_slice::<serde_json::Value>(&fragment).is_err() {
            return Err(Error::Other(format!(
                "refuse to confirm stats jsonl {}: trailing fragment is not a complete JSON record",
                path.display()
            )));
        }
        file.write_all(b"\n")?;
    }
    file.sync_all()?;
    // 文件内容 durable 之后还要确认目录项：首次创建时父目录同步可能失败，
    // 重放已有文件不能只凭“文件已存在”跳过。
    crate::storage::sync_parent(path)?;
    Ok(())
}

/// 追加一批已序列化的 JSONL 行，每行补一个换行，并完成持久化。
///
/// - 末尾若是完整记录但缺换行，只补分隔符，不覆盖任何已有字节；
/// - 末尾是无法解析成 JSON 的残片时返回错误，不把残片固化成一条坏记录；
/// - `lines` 为空时仍对已有文件执行一次分隔符修复与 `sync_all`，让上一次
///   “写成功但同步失败”的事实得到确认；文件本来不存在则什么都不做。
pub(crate) fn append_jsonl_lines(path: &Path, lines: &[String]) -> Result<()> {
    append_jsonl_lines_with(path, lines, true)
}

/// 追加但不立即 `sync_all`，由调用方在整批末尾统一确认。
pub(crate) fn append_jsonl_lines_deferred(path: &Path, lines: &[String]) -> Result<()> {
    append_jsonl_lines_with(path, lines, false)
}

fn append_jsonl_lines_with(path: &Path, lines: &[String], sync: bool) -> Result<()> {
    let existed = path.exists();
    if lines.is_empty() && !existed {
        return Ok(());
    }
    if !existed {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    let len = file.metadata()?.len();
    let fragment = read_trailing_fragment_bytes(&mut file, len)?;
    if !fragment.is_empty() && serde_json::from_slice::<serde_json::Value>(&fragment).is_err() {
        return Err(Error::Other(format!(
            "refuse to append stats jsonl {}: trailing fragment is not a complete JSON record",
            path.display()
        )));
    }
    let mut buffer: Vec<u8> = Vec::new();
    if !fragment.is_empty() {
        buffer.push(b'\n');
    }
    for line in lines {
        buffer.extend_from_slice(line.as_bytes());
        buffer.push(b'\n');
    }
    if !buffer.is_empty() {
        file.write_all(&buffer)?;
    }
    if sync {
        file.sync_all()?;
    }
    drop(file);
    // 每次同步写入都确认父目录项：首次创建时父目录同步可能失败，
    // 之后重放已有文件也必须补上，不能只凭文件已存在就跳过。
    if sync {
        crate::storage::sync_parent(path)?;
    }
    Ok(())
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
        let error = match read_jsonl::<TestEvent>(&path) {
            Ok(_) => panic!("strict read should reject invalid JSON"),
            Err(error) => error.to_string(),
        };
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
    fn empty_append_confirms_existing_file() {
        let path = temp_file("confirm");
        fs::write(&path, "{\"event_id\":\"a\",\"value\":1}").unwrap();
        append_jsonl_lines(&path, &[]).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "{\"event_id\":\"a\",\"value\":1}\n");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn repair_truncates_proven_prefix_only() {
        let path = temp_file("repair");
        let expected = "{\"event_id\":\"b\",\"value\":2}".to_string();
        fs::write(&path, "{\"event_id\":\"a\",\"value\":1}\n{\"event_i").unwrap();
        repair_tail_fragment::<TestEvent>(&path, std::slice::from_ref(&expected)).unwrap();
        append_jsonl_lines(&path, &[expected]).unwrap();
        let records = read_jsonl::<TestEvent>(&path).unwrap();
        assert_eq!(records.len(), 2);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn repair_handles_multibyte_fragment_cut_mid_character() {
        let path = temp_file("utf8");
        let expected = "{\"event_id\":\"中\",\"value\":2}".to_string();
        // 截断在“中”的 UTF-8 字节中间，旧实现会在 from_utf8 处直接报错。
        let cut = expected.find('中').unwrap() + 1;
        fs::write(&path, &expected.as_bytes()[..cut]).unwrap();
        read_jsonl_ids_repairing_tail::<TestEvent, _>(
            &path,
            |event| event.event_id.as_str(),
            std::slice::from_ref(&expected),
        )
        .unwrap();
        append_jsonl_lines(&path, &[expected]).unwrap();
        let records = read_jsonl::<TestEvent>(&path).unwrap();
        assert_eq!(records.len(), 1);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn id_payload_mismatch_is_rejected() {
        let path = temp_file("mismatch");
        fs::write(&path, "{\"event_id\":\"a\",\"value\":1}\n").unwrap();
        let expected = "{\"event_id\":\"a\",\"value\":9}".to_string();
        let error = read_jsonl_ids_repairing_tail::<TestEvent, _>(
            &path,
            |event| event.event_id.as_str(),
            std::slice::from_ref(&expected),
        )
        .unwrap_err();
        assert!(error.to_string().contains("different payload"), "{error}");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
