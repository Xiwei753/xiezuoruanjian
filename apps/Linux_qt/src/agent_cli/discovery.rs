// =============================================================================
// discovery.rs — 本机可执行文件发现
// =============================================================================
//
// Issue #1014：不维护固定 CLI 列表，也不假设固定安装目录。发现来源包括：
//   - 用户手工添加的路径；
//   - npm / pnpm / bun / pipx / cargo 的真实安装清单；
//   - XDG applications/*.desktop 的 Exec / TryExec；
//   - PATH 中所有可执行文件；
//   - rpm / dpkg / pacman 的包归属元数据（只对已匹配的少数文件查询）。
//
// 发现阶段不执行未知程序的 `--version`。那个动作只在 probe 阶段、且已有
// 声明式描述匹配后发生。
// =============================================================================

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::inventory::DiscoveredExecutable;
use super::probe::run_with_timeout;

const INVENTORY_TIMEOUT: Duration = Duration::from_secs(5);

pub fn collect_installed_executables(manual_paths: &[String]) -> Vec<DiscoveredExecutable> {
    let mut collector = ExecutableCollector::default();

    for path in manual_paths {
        collector.add_manual(path);
    }
    collector.add_npm_inventory();
    collector.add_pnpm_inventory();
    collector.add_bun_inventory();
    collector.add_pipx_inventory();
    collector.add_cargo_inventory();
    collector.add_desktop_entries();
    collector.add_path_entries();

    collector.finish()
}

#[derive(Default)]
struct ExecutableCollector {
    entries: Vec<DiscoveredExecutable>,
    by_key: HashMap<String, usize>,
}

impl ExecutableCollector {
    fn push(&mut self, mut executable: DiscoveredExecutable) {
        if !is_executable_file(Path::new(&executable.launch_path)) {
            return;
        }
        let key = executable.dedupe_key();
        if let Some(index) = self.by_key.get(&key).copied() {
            let existing = &mut self.entries[index];
            if existing.package_id.is_none() {
                existing.package_id = executable.package_id.take();
            }
            if existing.version.is_none() {
                existing.version = executable.version.take();
            }
            if !existing.source.contains(&executable.source) {
                existing.source = format!("{} + {}", existing.source, executable.source);
            }
            return;
        }
        let index = self.entries.len();
        self.by_key.insert(key, index);
        self.entries.push(executable);
    }

    fn add_manual(&mut self, path: &str) {
        let path = PathBuf::from(path);
        if path.is_dir() {
            self.scan_directory(&path, "manual", None);
        } else if let Some(name) = executable_name(&path) {
            self.push(DiscoveredExecutable::new(
                name,
                path.to_string_lossy().into_owned(),
                "manual",
            ));
        }
    }

    fn add_path_entries(&mut self) {
        let Some(path) = std::env::var_os("PATH") else {
            return;
        };
        for directory in std::env::split_paths(&path) {
            self.scan_directory(&directory, "PATH", None);
        }
    }

    fn scan_directory(&mut self, directory: &Path, source: &str, package: Option<(&str, &str)>) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_executable_file(&path) {
                continue;
            }
            let Some(name) = executable_name(&path) else {
                continue;
            };
            let mut executable =
                DiscoveredExecutable::new(name, path.to_string_lossy().into_owned(), source);
            if let Some((package_id, version)) = package {
                executable.package_id = Some(package_id.to_string());
                executable.version = Some(version.to_string());
            }
            self.push(executable);
        }
    }

    fn add_npm_inventory(&mut self) {
        if find_program("npm").is_none() {
            return;
        }
        let Some(prefix) = command_line("npm", &["prefix", "-g"]) else {
            return;
        };
        let bin_dir = PathBuf::from(prefix.trim()).join("bin");
        let root_dir = command_line("npm", &["root", "-g"])
            .map(|value| PathBuf::from(value.trim()))
            .filter(|path| path.is_dir())
            .unwrap_or_else(|| PathBuf::from(prefix.trim()).join("lib/node_modules"));
        let versions = npm_versions(Some(&root_dir));
        self.scan_package_bin_dir(&bin_dir, "npm", &versions);
    }

    fn add_pnpm_inventory(&mut self) {
        if find_program("pnpm").is_none() {
            return;
        }
        let Some(bin_dir) = command_line("pnpm", &["bin", "-g"]) else {
            return;
        };
        let root_dir = command_line("pnpm", &["root", "-g"])
            .map(|value| PathBuf::from(value.trim()))
            .filter(|path| path.is_dir());
        let versions = pnpm_versions(root_dir.as_deref());
        self.scan_package_bin_dir(Path::new(bin_dir.trim()), "pnpm", &versions);
    }

    fn add_bun_inventory(&mut self) {
        if find_program("bun").is_none() {
            return;
        }
        let Some(bin_dir) = command_line("bun", &["pm", "bin", "-g"]) else {
            return;
        };
        self.scan_package_bin_dir(Path::new(bin_dir.trim()), "bun", &HashMap::new());
    }

    fn add_pipx_inventory(&mut self) {
        if find_program("pipx").is_none() {
            return;
        }
        let Some(json) = command_line("pipx", &["list", "--json"]) else {
            return;
        };
        let Ok(document) = serde_json::from_str::<Value>(&json) else {
            return;
        };
        let bin_dir = command_line("pipx", &["environment", "--value", "PIPX_BIN_DIR"])
            .map(|value| PathBuf::from(value.trim()))
            .filter(|path| !path.as_os_str().is_empty());
        let Some(venvs) = document.get("venvs").and_then(Value::as_object) else {
            return;
        };
        for venv in venvs.values() {
            let Some(main) = venv
                .pointer("/metadata/main_package")
                .and_then(Value::as_object)
            else {
                continue;
            };
            let package_name = main
                .get("package_name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let version = main
                .get("package_version")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(apps) = main.get("apps").and_then(Value::as_array) else {
                continue;
            };
            for app in apps.iter().filter_map(Value::as_str) {
                let path = match bin_dir.as_ref() {
                    Some(directory) => directory.join(app),
                    None => match find_program(app) {
                        Some(path) => path,
                        None => continue,
                    },
                };
                if !is_executable_file(&path) {
                    continue;
                }
                let mut executable = DiscoveredExecutable::new(
                    app.to_string(),
                    path.to_string_lossy().into_owned(),
                    "pipx",
                );
                if !package_name.is_empty() {
                    executable.package_id = Some(package_name.to_string());
                }
                if !version.is_empty() {
                    executable.version = Some(version.to_string());
                }
                self.push(executable);
            }
        }
    }

    fn add_cargo_inventory(&mut self) {
        if find_program("cargo").is_none() {
            return;
        }
        let Some(listing) = command_line("cargo", &["install", "--list"]) else {
            return;
        };
        let mut package_name: Option<String> = None;
        let mut version: Option<String> = None;
        for line in listing.lines() {
            if line.starts_with(char::is_whitespace) {
                let name = line.trim();
                if name.is_empty() {
                    continue;
                }
                // 不假设 ~/.cargo/bin 等固定安装目录；只在 PATH 能实际解析时
                // 记录 cargo 安装信息，保留命中的真实启动入口。
                if let (Some(package), Some(version), Some(path)) = (
                    package_name.as_deref(),
                    version.as_deref(),
                    find_program(name),
                ) {
                    let mut executable = DiscoveredExecutable::new(
                        name.to_string(),
                        path.to_string_lossy().into_owned(),
                        "cargo",
                    );
                    executable.package_id = Some(package.to_string());
                    executable.version = Some(version.to_string());
                    self.push(executable);
                }
                continue;
            }
            let trimmed = line.trim().trim_end_matches(':');
            let mut parts = trimmed.split_whitespace();
            package_name = parts.next().map(str::to_string);
            version = parts
                .next()
                .map(|value| value.trim_start_matches('v').to_string());
        }
    }

    fn scan_package_bin_dir(
        &mut self,
        bin_dir: &Path,
        source: &str,
        versions: &HashMap<String, (String, String)>,
    ) {
        let Ok(entries) = fs::read_dir(bin_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_executable_file(&path) {
                continue;
            }
            let Some(name) = executable_name(&path) else {
                continue;
            };
            let mut executable = DiscoveredExecutable::new(
                name.clone(),
                path.to_string_lossy().into_owned(),
                source,
            );
            if let Some((package, version)) = versions.get(&name) {
                executable.package_id = Some(package.clone());
                executable.version = Some(version.clone());
            }
            self.push(executable);
        }
    }

    fn add_desktop_entries(&mut self) {
        for data_dir in xdg_data_dirs() {
            let applications = data_dir.join("applications");
            let Ok(entries) = fs::read_dir(&applications) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
                    continue;
                }
                let Some(commands) = desktop_commands(&path) else {
                    continue;
                };
                for command in commands {
                    if let Some(program) = find_program(&command) {
                        if let Some(name) = executable_name(&program) {
                            self.push(DiscoveredExecutable::new(
                                name,
                                program.to_string_lossy().into_owned(),
                                "desktop",
                            ));
                        }
                    }
                }
            }
        }
    }

    fn finish(self) -> Vec<DiscoveredExecutable> {
        let mut entries = self.entries;
        entries.sort_by(|left, right| {
            left.executable_name
                .cmp(&right.executable_name)
                .then_with(|| left.launch_path.cmp(&right.launch_path))
        });
        entries
    }
}

/// 对已匹配的候选补充系统包归属。只在匹配之后调用，避免对 PATH 中每个
/// 文件都启动包管理器。
pub fn enrich_package_metadata(executable: &mut DiscoveredExecutable) {
    if executable.package_id.is_some() {
        return;
    }
    let candidates = [
        executable.launch_path.clone(),
        executable.canonical_path.clone().unwrap_or_default(),
    ];
    for path in candidates.iter().filter(|path| !path.is_empty()) {
        if find_program("rpm").is_some() {
            if let Some((package, version)) =
                rpm_query(&["-qf", "--qf", "%{NAME}\t%{VERSION}", path])
            {
                executable.package_id = Some(package);
                executable.version = Some(version);
                executable.source = format!("{} (rpm)", executable.source);
                return;
            }
        }
        if find_program("dpkg-query").is_some() {
            if let Some(package) = dpkg_owner(path) {
                executable.version = dpkg_version(&package);
                executable.package_id = Some(package);
                executable.source = format!("{} (dpkg)", executable.source);
                return;
            }
        }
        if find_program("pacman").is_some() {
            if let Some(package) = pacman_owner(path) {
                executable.version = pacman_version(&package);
                executable.package_id = Some(package);
                executable.source = format!("{} (pacman)", executable.source);
                return;
            }
        }
    }
}

pub fn find_program(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let path = PathBuf::from(name);
        return is_executable_file(&path).then_some(path);
    }
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

fn executable_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
}

fn command_line(program: &str, args: &[&str]) -> Option<String> {
    let output = run_with_timeout(program, args, INVENTORY_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn xdg_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(data_home));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }
    let data_dirs =
        std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(std::env::split_paths(&data_dirs));
    dirs
}

fn desktop_commands(path: &Path) -> Option<Vec<String>> {
    let contents = fs::read_to_string(path).ok()?;
    let mut in_desktop_entry = false;
    let mut exec = None;
    let mut try_exec = None;
    let mut hidden = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        if let Some(value) = line.strip_prefix("Exec=") {
            exec = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("TryExec=") {
            try_exec = Some(value.to_string());
        } else if line == "Hidden=true" {
            hidden = true;
        }
    }
    if hidden {
        return None;
    }
    let mut commands = Vec::new();
    if let Some(exec) = exec {
        if let Some(program) = exec_program(&exec) {
            commands.push(program);
        }
    }
    if let Some(try_exec) = try_exec {
        let token = strip_field_codes(try_exec.trim());
        if !token.is_empty() {
            commands.push(token);
        }
    }
    Some(commands)
}

/// 从 Exec 行取出真正的可执行文件名。跳过 `env VAR=value` 前缀与
/// 字段码；不尝试解析 shell 语法。
fn exec_program(exec: &str) -> Option<String> {
    let tokens = tokenize_exec(exec);
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token == "env" {
            index += 1;
            while index < tokens.len() && tokens[index].contains('=') {
                index += 1;
            }
            continue;
        }
        if token.contains('=') && index == 0 {
            index += 1;
            continue;
        }
        let program = strip_field_codes(token);
        return (!program.is_empty()).then_some(program);
    }
    None
}

fn tokenize_exec(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(active) = quote {
            if character == active {
                quote = None;
            } else {
                current.push(character);
            }
            continue;
        }
        if character == '"' || character == '\'' {
            quote = Some(character);
        } else if character.is_whitespace() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn strip_field_codes(value: &str) -> String {
    let mut output = String::new();
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '%' {
            match characters.peek().copied() {
                Some('%') => {
                    let _ = characters.next();
                    output.push('%');
                }
                Some(_) => {
                    let _ = characters.next();
                }
                None => output.push('%'),
            }
        } else {
            output.push(character);
        }
    }
    output
}

fn npm_versions(root_dir: Option<&Path>) -> HashMap<String, (String, String)> {
    let mut versions = HashMap::new();
    let Some(json) = command_line("npm", &["ls", "-g", "--depth=0", "--json"]) else {
        return versions;
    };
    let Ok(document) = serde_json::from_str::<Value>(&json) else {
        return versions;
    };
    collect_npm_dependencies(&document, root_dir, &mut versions);
    versions
}

fn pnpm_versions(root_dir: Option<&Path>) -> HashMap<String, (String, String)> {
    let mut versions = HashMap::new();
    let Some(json) = command_line("pnpm", &["list", "-g", "--depth=0", "--json"]) else {
        return versions;
    };
    let Ok(document) = serde_json::from_str::<Value>(&json) else {
        return versions;
    };
    match &document {
        Value::Array(items) => {
            for item in items {
                collect_npm_dependencies(item, root_dir, &mut versions);
            }
        }
        _ => collect_npm_dependencies(&document, root_dir, &mut versions),
    }
    versions
}

fn collect_npm_dependencies(
    document: &Value,
    root_dir: Option<&Path>,
    output: &mut HashMap<String, (String, String)>,
) {
    let Some(dependencies) = document.get("dependencies").and_then(Value::as_object) else {
        return;
    };
    for (package, metadata) in dependencies {
        let Some(version) = metadata.get("version").and_then(Value::as_str) else {
            continue;
        };
        let mut bin_names = root_dir
            .map(|root| package_bin_names(root, package))
            .unwrap_or_default();
        if bin_names.is_empty() {
            // npm ls 不提供 bin map；回退到包名 basename 只作说明，实际
            // 启动入口仍来自 bin 目录扫描结果。
            bin_names.push(package.rsplit('/').next().unwrap_or(package).to_string());
        }
        for bin_name in bin_names {
            output.insert(bin_name, (package.clone(), version.to_string()));
        }
    }
}

fn package_bin_names(root_dir: &Path, package: &str) -> Vec<String> {
    let path = root_dir.join(package).join("package.json");
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(document) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    match document.get("bin") {
        Some(Value::Object(bins)) => bins
            .keys()
            .filter(|name| !name.is_empty())
            .cloned()
            .collect(),
        Some(Value::String(_)) => vec![package.rsplit('/').next().unwrap_or(package).to_string()],
        _ => Vec::new(),
    }
}

fn rpm_query(args: &[&str]) -> Option<(String, String)> {
    let output = run_with_timeout("rpm", args, INVENTORY_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout);
    let mut parts = line.trim().splitn(2, '\t');
    let package = parts.next()?.trim();
    let version = parts.next().unwrap_or_default().trim();
    (!package.is_empty()).then(|| (package.to_string(), version.to_string()))
}

fn dpkg_owner(path: &str) -> Option<String> {
    let output = run_with_timeout("dpkg-query", &["-S", path], INVENTORY_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first_line = text.lines().next()?;
    let package = first_line.split(':').next()?.trim();
    (!package.is_empty()).then(|| package.to_string())
}

fn dpkg_version(package: &str) -> Option<String> {
    let output = run_with_timeout(
        "dpkg-query",
        &["-W", "-f=${Version}", package],
        INVENTORY_TIMEOUT,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!version.is_empty()).then_some(version)
}

fn pacman_owner(path: &str) -> Option<String> {
    let output = run_with_timeout("pacman", &["-Qo", path], INVENTORY_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next()?;
    let package = line.split_whitespace().next()?.trim();
    (!package.is_empty()).then(|| package.to_string())
}

fn pacman_version(package: &str) -> Option<String> {
    let output = run_with_timeout("pacman", &["-Q", package], INVENTORY_TIMEOUT).ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let version = text.split_whitespace().nth(1)?.to_string();
    (!version.is_empty()).then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_line_skips_env_and_field_codes() {
        assert_eq!(
            exec_program("env FOO=bar /usr/bin/codex %U"),
            Some("/usr/bin/codex".to_string())
        );
        assert_eq!(
            exec_program("\"/opt/My App/bin/tool\" --flag"),
            Some("/opt/My App/bin/tool".to_string())
        );
    }

    #[test]
    fn field_code_stripping_handles_literal_percent() {
        assert_eq!(strip_field_codes("tool %%u %F"), "tool %u ");
    }
}
