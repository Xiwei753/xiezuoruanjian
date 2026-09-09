use std::path::Path;

/// 同步路径验证类型——远端路径进入 Core 前的唯一安全边界。
///
/// 远端 path 可以携带 `../`、绝对路径、Windows 盘符/UNC 或反斜杠穿越，
/// 直接 `sync_root.join(path)` 会写出 sync 根目录。
/// `ValidatedSyncPath` 在构造时拒绝所有非法路径，
/// 通过校验后内部存为标准化的 `/` 分隔相对路径。
///
/// ## 不变量
///
/// - 内部字符串始终使用 `/` 分隔符（`\` 在构造时统一转换）
/// - 不以 `/` 开头（非绝对路径）
/// - 不含路径组件 `.` 或 `..`
/// - 每个路径组件都是非空的普通名称（无 Windows prefix/UNC）
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ValidatedSyncPath(String);

/// 路径验证失败的类型化错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncPathError {
    /// 路径是绝对路径（以 `/` 开头，或 Windows 盘符如 `C:\`）
    AbsolutePath,
    /// 路径包含 `..` 组件（目录遍历）
    DirectoryTraversal,
    /// 路径包含空组件（连续 `/` 或尾部 `/`）
    EmptyComponent,
    /// 路径包含 `.` 组件
    DotComponent,
    /// Windows UNC 路径（`\\server\share`）
    UncPath,
    /// 路径包含 Windows 盘符前缀（`C:`、`\\?\` 等）
    WindowsPrefix,
}

impl std::fmt::Display for SyncPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AbsolutePath => write!(f, "absolute path rejected"),
            Self::DirectoryTraversal => write!(f, "directory traversal (..) rejected"),
            Self::EmptyComponent => write!(f, "empty path component rejected"),
            Self::DotComponent => write!(f, "dot (.) component rejected"),
            Self::UncPath => write!(f, "UNC path rejected"),
            Self::WindowsPrefix => write!(f, "Windows prefix path rejected"),
        }
    }
}

impl std::error::Error for SyncPathError {}

impl std::convert::From<SyncPathError> for crate::Error {
    fn from(e: SyncPathError) -> Self {
        crate::Error::Io(std::io::Error::other(format!("sync path validation: {e}")))
    }
}

impl ValidatedSyncPath {
    /// 验证并构造安全的同步路径。
    ///
    /// 1. 将 `\` 统一成 `/`
    /// 2. 逐组件检查：绝对路径、`.`、`..`、空组件、Windows 前缀全部拒绝
    /// 3. 通过后返回标准化的相对路径
    pub fn new(raw: &str) -> Result<Self, SyncPathError> {
        // 1. 拒绝 Windows UNC 路径（`\\server\share` 或 `\\?\...`）——在转换分隔符前检查原始输入
        if raw.starts_with("\\\\") {
            return Err(SyncPathError::UncPath);
        }

        // 2. 统一分隔符
        let normalized = raw.replace('\\', "/");

        // 3. 拒绝绝对路径
        if normalized.starts_with('/') {
            return Err(SyncPathError::AbsolutePath);
        }

        // 4. 拒绝 Windows 盘符（如 `C:/...`）
        {
            let bytes = normalized.as_bytes();
            if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
                return Err(SyncPathError::WindowsPrefix);
            }
        }

        // 5. 逐组件检查
        for component in normalized.split('/') {
            match component {
                "" => return Err(SyncPathError::EmptyComponent),
                "." => return Err(SyncPathError::DotComponent),
                ".." => return Err(SyncPathError::DirectoryTraversal),
                _ => {}
            }
        }

        Ok(Self(normalized))
    }

    /// 把验证过的相对路径拼接到 `sync_root` 下，得到最终本地路径。
    ///
    /// 不做任何额外的路径验证——`ValidatedSyncPath` 已保证安全。
    pub fn join_under(&self, sync_root: &Path) -> std::path::PathBuf {
        sync_root.join(&self.0)
    }

    /// 返回内部标准化的路径字符串。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ValidatedSyncPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// 批量验证路径列表，返回 `(通过的路径, 失败的路径+错误)` 分组。
#[allow(clippy::type_complexity)]
pub fn partition_validated_paths(
    raw_paths: &[String],
) -> (
    Vec<(ValidatedSyncPath, String)>,
    Vec<(String, SyncPathError)>,
) {
    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for raw in raw_paths {
        match ValidatedSyncPath::new(raw) {
            Ok(vp) => valid.push((vp, raw.clone())),
            Err(e) => invalid.push((raw.clone(), e)),
        }
    }
    (valid, invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_relative_path_accepted() {
        let vp = ValidatedSyncPath::new("volumes/1/chapter.md").unwrap();
        assert_eq!(vp.as_str(), "volumes/1/chapter.md");
    }

    #[test]
    fn chinese_filename_accepted() {
        let vp = ValidatedSyncPath::new("volumes/第一章/正文.md").unwrap();
        assert_eq!(vp.as_str(), "volumes/第一章/正文.md");
    }

    #[test]
    fn backslash_normalized() {
        let vp = ValidatedSyncPath::new("volumes\\1\\chapter.md").unwrap();
        assert_eq!(vp.as_str(), "volumes/1/chapter.md");
    }

    #[test]
    fn single_file_accepted() {
        let vp = ValidatedSyncPath::new("project.json").unwrap();
        assert_eq!(vp.as_str(), "project.json");
    }

    #[test]
    fn relative_dotdot_traversal_rejected() {
        assert_eq!(
            ValidatedSyncPath::new("../project.json").unwrap_err(),
            SyncPathError::DirectoryTraversal
        );
    }

    #[test]
    fn nested_dotdot_traversal_rejected() {
        assert_eq!(
            ValidatedSyncPath::new("volumes/../../outside/chapter.md").unwrap_err(),
            SyncPathError::DirectoryTraversal
        );
    }

    #[test]
    fn absolute_path_rejected() {
        assert_eq!(
            ValidatedSyncPath::new("/etc/passwd").unwrap_err(),
            SyncPathError::AbsolutePath
        );
    }

    #[test]
    fn windows_drive_letter_rejected() {
        assert_eq!(
            ValidatedSyncPath::new("C:/Users/test/file.txt").unwrap_err(),
            SyncPathError::WindowsPrefix
        );
    }

    #[test]
    fn windows_unc_rejected() {
        assert_eq!(
            ValidatedSyncPath::new("\\\\server\\share\\file.txt").unwrap_err(),
            SyncPathError::UncPath
        );
    }

    #[test]
    fn windows_prefix_rejected() {
        assert_eq!(
            ValidatedSyncPath::new("\\\\?\\C:\\file.txt").unwrap_err(),
            SyncPathError::UncPath
        );
    }

    #[test]
    fn empty_component_rejected() {
        assert_eq!(
            ValidatedSyncPath::new("volumes//chapter.md").unwrap_err(),
            SyncPathError::EmptyComponent
        );
    }

    #[test]
    fn trailing_slash_empty_component_rejected() {
        assert_eq!(
            ValidatedSyncPath::new("volumes/").unwrap_err(),
            SyncPathError::EmptyComponent
        );
    }

    #[test]
    fn dot_component_rejected() {
        assert_eq!(
            ValidatedSyncPath::new("./file.txt").unwrap_err(),
            SyncPathError::DotComponent
        );
    }

    #[test]
    fn single_dotdot_rejected() {
        assert_eq!(
            ValidatedSyncPath::new("..").unwrap_err(),
            SyncPathError::DirectoryTraversal
        );
    }

    #[test]
    fn join_under_works() {
        let vp = ValidatedSyncPath::new("volumes/1/chapter.md").unwrap();
        let full = vp.join_under(Path::new("/sync/root"));
        assert_eq!(full, Path::new("/sync/root/volumes/1/chapter.md"));
    }

    #[test]
    fn partition_validated_paths_split() {
        let paths = vec![
            "ok/file.md".to_string(),
            "../bad.md".to_string(),
            "/abs/path.md".to_string(),
            "also/ok.md".to_string(),
        ];
        let (valid, invalid) = partition_validated_paths(&paths);
        assert_eq!(valid.len(), 2);
        assert_eq!(invalid.len(), 2);
        assert_eq!(valid[0].0.as_str(), "ok/file.md");
        assert_eq!(valid[1].0.as_str(), "also/ok.md");
        assert_eq!(invalid[0].0, "../bad.md");
        assert_eq!(invalid[1].0, "/abs/path.md");
    }
}
