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
                let candidate = path.join(name);
                if is_executable_file(&candidate) {
                    push(&mut entries, &mut seen, candidate, "manual");
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
        let candidate = directory.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
        && path
            .metadata()
            .map(|metadata| {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            })
            .unwrap_or(false)
}

fn executable_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}
