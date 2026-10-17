//! HarmonyOS 安全存储实现（Issue #924）。
//!
//! 基于 HUKS 托管的设备级不可导出 AES-256 密钥做 AES-GCM 信封加密：
//! - 应用私有目录只保存 `版本 + nonce + tag + 密文` 信封，不存明文、不存密钥；
//! - 每个 key 独立随机 nonce，每次写入重新加密；
//! - HUKS 不可用或加解密失败时明确返回错误，绝不降级写回明文文件。
//!
//! 密钥与加解密会话全部由 `huks_storage.c`（HUKS NDK C 接口）负责，
//! 本模块不接触 HUKS 类型，只处理信封文件与 `SecureStorage` 契约。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use writer_platform_api::SecureStorage;

/// AES-GCM nonce 长度（字节），与 C shim 契约一致。
const NONCE_LEN: usize = 12;
/// AES-GCM tag 长度（字节），与 C shim 契约一致。
const TAG_LEN: usize = 16;
/// 信封格式版本。
const ENVELOPE_VERSION: u8 = 1;
/// 信封头长度：版本 + nonce + tag。
const ENVELOPE_HEADER_LEN: usize = 1 + NONCE_LEN + TAG_LEN;

#[cfg(target_env = "ohos")]
mod platform {
    use std::os::raw::c_char;

    use super::{NONCE_LEN, TAG_LEN};

    /// C shim 错误描述缓冲长度。
    const ERROR_BUFFER_LEN: usize = 256;

    extern "C" {
        fn sujian_huks_probe(error_out: *mut c_char, error_out_len: u32) -> i32;
        fn sujian_huks_encrypt(
            plaintext: *const u8,
            plaintext_len: u32,
            nonce_out: *mut u8,
            nonce_cap: u32,
            ciphertext_out: *mut u8,
            ciphertext_len: *mut u32,
            error_out: *mut c_char,
            error_out_len: u32,
        ) -> i32;
        fn sujian_huks_decrypt(
            nonce: *const u8,
            nonce_len: u32,
            ciphertext: *const u8,
            ciphertext_len: u32,
            plaintext_out: *mut u8,
            plaintext_len: *mut u32,
            error_out: *mut c_char,
            error_out_len: u32,
        ) -> i32;
    }

    fn error_message(buffer: &[u8]) -> String {
        let end = buffer
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(buffer.len());
        String::from_utf8_lossy(&buffer[..end]).into_owned()
    }

    fn describe(code: i32, buffer: &[u8]) -> String {
        let message = error_message(buffer);
        if message.is_empty() {
            format!("HUKS error code {}", code)
        } else {
            format!("HUKS error code {}: {}", code, message)
        }
    }

    /// 探测 HUKS 可用性：确保设备级密钥存在并做一次加解密回环自检。
    pub(super) fn probe() -> Result<(), String> {
        let mut error = vec![0_u8; ERROR_BUFFER_LEN];
        // SAFETY: error 是长度与 ERROR_BUFFER_LEN 一致的本地 Vec，指针在调用期间有效。
        let code =
            unsafe { sujian_huks_probe(error.as_mut_ptr() as *mut c_char, error.len() as u32) };
        if code == 0 {
            Ok(())
        } else {
            Err(describe(code, &error))
        }
    }

    /// AES-GCM 加密，返回 `(nonce, 密文 || tag)`。
    pub(super) fn encrypt(plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
        if plaintext.len() > u32::MAX as usize {
            return Err("secret is too large for HUKS".to_string());
        }
        let mut nonce = vec![0_u8; NONCE_LEN];
        let mut ciphertext = vec![0_u8; plaintext.len() + TAG_LEN];
        let mut ciphertext_len = ciphertext.len() as u32;
        let mut error = vec![0_u8; ERROR_BUFFER_LEN];
        // SAFETY: 所有指针指向在调用期间有效且容量足够的本地缓冲；
        // ciphertext_len 入口为 ciphertext 的容量，出口由 C shim 写实际长度。
        let code = unsafe {
            sujian_huks_encrypt(
                plaintext.as_ptr(),
                plaintext.len() as u32,
                nonce.as_mut_ptr(),
                nonce.len() as u32,
                ciphertext.as_mut_ptr(),
                &mut ciphertext_len,
                error.as_mut_ptr() as *mut c_char,
                error.len() as u32,
            )
        };
        if code != 0 {
            return Err(describe(code, &error));
        }
        ciphertext.truncate(ciphertext_len as usize);
        Ok((nonce, ciphertext))
    }

    /// AES-GCM 解密，`sealed` 为 `密文 || tag`。
    pub(super) fn decrypt(nonce: &[u8], sealed: &[u8]) -> Result<Vec<u8>, String> {
        if nonce.len() != NONCE_LEN || sealed.len() < TAG_LEN {
            return Err("invalid sealed secret layout".to_string());
        }
        let mut plaintext = vec![0_u8; sealed.len()];
        let mut plaintext_len = plaintext.len() as u32;
        let mut error = vec![0_u8; ERROR_BUFFER_LEN];
        // SAFETY: 所有指针指向在调用期间有效且容量足够的本地缓冲；
        // plaintext_len 入口为 plaintext 的容量，出口由 C shim 写实际长度。
        let code = unsafe {
            sujian_huks_decrypt(
                nonce.as_ptr(),
                nonce.len() as u32,
                sealed.as_ptr(),
                sealed.len() as u32,
                plaintext.as_mut_ptr(),
                &mut plaintext_len,
                error.as_mut_ptr() as *mut c_char,
                error.len() as u32,
            )
        };
        if code != 0 {
            return Err(describe(code, &error));
        }
        plaintext.truncate(plaintext_len as usize);
        Ok(plaintext)
    }
}

#[cfg(not(target_env = "ohos"))]
mod platform {
    const UNAVAILABLE: &str = "HUKS secure storage is only available on HarmonyOS targets";

    pub(super) fn probe() -> Result<(), String> {
        Err(UNAVAILABLE.to_string())
    }

    pub(super) fn encrypt(_plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
        Err(UNAVAILABLE.to_string())
    }

    pub(super) fn decrypt(_nonce: &[u8], _sealed: &[u8]) -> Result<Vec<u8>, String> {
        Err(UNAVAILABLE.to_string())
    }
}

/// 基于 HUKS 的 `SecureStorage`：密文信封存放在 `app_data_root/secure_storage/`。
pub struct HarmonyHuksSecureStorage {
    directory: PathBuf,
}

impl HarmonyHuksSecureStorage {
    /// 建立安全存储目录并创建实例。**不在启动时探测 HUKS**：安全能力不可用时
    /// 只应让凭证/同步操作失败，离线写作照常（Issue #924 复核）。
    /// 失败时返回明确错误，不回退明文路径。
    pub fn new(app_data_root: &Path) -> Result<Self, String> {
        if app_data_root.as_os_str().is_empty() {
            return Err("app data root is empty".to_string());
        }
        let directory = app_data_root.join("secure_storage");
        fs::create_dir_all(&directory)
            .map_err(|error| format!("create secure storage directory failed: {}", error))?;
        Ok(Self { directory })
    }

    /// 显式安全能力诊断：密钥生成 + 加解密回环自检。
    /// 供诊断入口按需调用，不绑定 Core 启动（Issue #924 复核）。
    pub fn probe_security_capability(&self) -> Result<(), String> {
        platform::probe()
    }

    /// key → 信封文件名：对 key 原始字节做十六进制编码，避免路径穿越与非法字符。
    fn envelope_path(&self, key: &str) -> PathBuf {
        let encoded: String = key
            .as_bytes()
            .iter()
            .map(|byte| format!("{:02x}", byte))
            .collect();
        self.directory.join(format!("{}.bin", encoded))
    }
}

impl SecureStorage for HarmonyHuksSecureStorage {
    fn get_secret(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        if key.is_empty() {
            return Err("secure storage key is empty".to_string());
        }
        let path = self.envelope_path(key);
        let envelope = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("read secret envelope failed: {}", error)),
        };
        let (nonce, tag, ciphertext) = split_envelope(&envelope)?;
        let mut sealed = Vec::with_capacity(ciphertext.len() + TAG_LEN);
        sealed.extend_from_slice(ciphertext);
        sealed.extend_from_slice(tag);
        Ok(Some(platform::decrypt(nonce, &sealed)?))
    }

    fn set_secret(&self, key: &str, value: &[u8]) -> Result<(), String> {
        if key.is_empty() {
            return Err("secure storage key is empty".to_string());
        }
        let (nonce, sealed) = platform::encrypt(value)?;
        if sealed.len() < TAG_LEN {
            return Err("HUKS returned a truncated ciphertext".to_string());
        }
        let (ciphertext, tag) = sealed.split_at(sealed.len() - TAG_LEN);
        let envelope = build_envelope(&nonce, tag, ciphertext);
        write_atomically(&self.envelope_path(key), &envelope)
    }

    fn delete_secret(&self, key: &str) -> Result<(), String> {
        if key.is_empty() {
            return Err("secure storage key is empty".to_string());
        }
        let path = self.envelope_path(key);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("delete secret envelope failed: {}", error)),
        }
    }
}

/// 组装 `版本 + nonce + tag + 密文` 信封。
fn build_envelope(nonce: &[u8], tag: &[u8], ciphertext: &[u8]) -> Vec<u8> {
    let mut envelope = Vec::with_capacity(ENVELOPE_HEADER_LEN + ciphertext.len());
    envelope.push(ENVELOPE_VERSION);
    envelope.extend_from_slice(nonce);
    envelope.extend_from_slice(tag);
    envelope.extend_from_slice(ciphertext);
    envelope
}

/// 校验并切分信封，返回 `(nonce, tag, 密文)`。
fn split_envelope(envelope: &[u8]) -> Result<(&[u8], &[u8], &[u8]), String> {
    // 密文至少包含一个 AE tag。
    if envelope.len() < ENVELOPE_HEADER_LEN + TAG_LEN {
        return Err("secret envelope is truncated".to_string());
    }
    if envelope[0] != ENVELOPE_VERSION {
        return Err(format!(
            "unsupported secret envelope version {}",
            envelope[0]
        ));
    }
    let nonce = &envelope[1..1 + NONCE_LEN];
    let tag = &envelope[1 + NONCE_LEN..ENVELOPE_HEADER_LEN];
    let ciphertext = &envelope[ENVELOPE_HEADER_LEN..];
    Ok((nonce, tag, ciphertext))
}

/// 原子写入信封：先写同目录临时文件（0600），再 rename 覆盖目标。
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let directory = path
        .parent()
        .ok_or_else(|| "secret envelope path has no parent".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "secret envelope file name is invalid".to_string())?;
    let temp_path = directory.join(format!(".{}.tmp", file_name));

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp_path)
        .map_err(|error| format!("create temp secret envelope failed: {}", error))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("set secret envelope permissions failed: {}", error))?;
    }

    file.write_all(bytes)
        .map_err(|error| format!("write secret envelope failed: {}", error))?;
    file.sync_all()
        .map_err(|error| format!("sync secret envelope failed: {}", error))?;
    drop(file);

    fs::rename(&temp_path, path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!("replace secret envelope failed: {}", error)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_layout_roundtrips() {
        let nonce = [7_u8; NONCE_LEN];
        let tag = [9_u8; TAG_LEN];
        // 真实密文至少含一个 AE tag（GCM 空明文也只产生 tag）。
        let ciphertext = [1_u8; TAG_LEN];
        let envelope = build_envelope(&nonce, &tag, &ciphertext);
        assert_eq!(envelope.len(), ENVELOPE_HEADER_LEN + ciphertext.len());
        let (parsed_nonce, parsed_tag, parsed_ciphertext) = split_envelope(&envelope)
            .unwrap_or_else(|error| panic!("split envelope failed: {error}"));
        assert_eq!(parsed_nonce, &nonce);
        assert_eq!(parsed_tag, &tag);
        assert_eq!(parsed_ciphertext, &ciphertext);
    }

    #[test]
    fn envelope_rejects_bad_version_and_truncation() {
        let mut envelope = build_envelope(&[0_u8; NONCE_LEN], &[0_u8; TAG_LEN], &[0_u8; TAG_LEN]);
        assert!(split_envelope(&envelope).is_ok());
        envelope[0] = 2;
        assert!(split_envelope(&envelope).is_err());
        let truncated = vec![ENVELOPE_VERSION; ENVELOPE_HEADER_LEN + TAG_LEN - 1];
        assert!(split_envelope(&truncated).is_err());
    }

    #[test]
    fn envelope_path_is_hex_encoded() {
        let storage = HarmonyHuksSecureStorage {
            directory: PathBuf::from("/data/secure"),
        };
        let path = storage.envelope_path("sync_token_global");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("73796e635f746f6b656e5f676c6f62616c.bin")
        );
    }
}
