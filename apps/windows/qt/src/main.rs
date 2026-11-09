#[cfg(windows)]
fn main() {
    sujian_windows_qt::windows_main::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("sujian-windows-qt 只能在 Windows 上运行。");
}
