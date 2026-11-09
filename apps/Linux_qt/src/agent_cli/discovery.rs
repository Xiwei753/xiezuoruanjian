// =============================================================================
// discovery.rs — 只发现受支持 CLI 的真实安装路径
// =============================================================================
//
// Issue #1022：不再遍历 npm / pnpm / bun / pipx / cargo 包清单，也不扫描
// 整个 PATH 目录。OpenCode 与 Codex 只按程序名在 PATH 中查找，用户可以
// 通过手工路径指定其它安装位置。
// =============================================================================

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::inventory::DiscoveredExecutable;

const SUPPORTED_PROGRAMS: &[&str] = &["opencode", "codex"];

pub fn collect_installed_executables(manual_paths: &[String]) -> Vec<DiscoveredExecutable> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for path in manual_paths {
        let path = PathBuf::from(path);
        if path.is_dir() {
            for name in SUPPORTED_PROGRAMS {
                for candidate in program_candidates(name) {
                    let candidate = path.join(candidate);
                    if is_executable_file(&candidate) {
                        push(&mut entries, &mut seen, candidate, "manual");
                    }
                }
            }
        } else if is_executable_file(&path) {
            push(&mut entries, &mut seen, path, "manual");
        }
    }

    for name in SUPPORTED_PROGRAMS {
        if let Some(path) = find_program(name) {
            push(&mut entries, &mut seen, path, "path");
        }
    }

    entries.sort_by(|left, right| {
        left.executable_name
            .cmp(&right.executable_name)
            .then_with(|| left.launch_path.cmp(&right.launch_path))
    });
    entries
}

fn push(
    entries: &mut Vec<DiscoveredExecutable>,
    seen: &mut HashSet<String>,
    path: PathBuf,
    source: &str,
) {
    let Some(name) = executable_name(&path) else {
        return;
    };
    let executable = DiscoveredExecutable::new(name, path.to_string_lossy().into_owned(), source);
    if seen.insert(executable.dedupe_key()) {
        entries.push(executable);
    }
}

pub fn find_program(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(name);
    if path.components().count() > 1 {
        return is_executable_file(&path).then_some(path);
    }
    let search_path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&search_path) {
        for candidate_name in program_candidates(name) {
            let candidate = directory.join(candidate_name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Windows 的 npm 全局命令通常落成 `.cmd` / `.bat`，原生安装则是 `.exe`。
/// Unix 仍按原始程序名查找，行为不变。
fn program_candidates(name: &str) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let path = Path::new(name);
        if path.extension().is_some() {
            return vec![path.to_path_buf()];
        }
        vec![
            PathBuf::from(format!("{name}.exe")),
            PathBuf::from(format!("{name}.cmd")),
            PathBuf::from(format!("{name}.bat")),
            PathBuf::from(name),
        ]
    }
    #[cfg(not(windows))]
    {
        vec![PathBuf::from(name)]
    }
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn executable_name(path: &Path) -> Option<String> {
    let name = path.file_name().and_then(|name| name.to_str())?;
    #[cfg(windows)]
    {
        let lower = name.to_ascii_lowercase();
        for extension in [".exe", ".cmd", ".bat"] {
            if lower.ends_with(extension) {
                return Some(name[..name.len() - extension.len()].to_string());
            }
        }
    }
    Some(name.to_string())
}
