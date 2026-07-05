//! Platform-specific input surface adapters.
//!
//! `qt_surface.rs` owns Qt event routing only.  Platform IME behavior lives in
//! cfg-selected adapters below so Windows pending-key state cannot affect Linux
//! fcitx5/ibus handling.

#[cfg(target_os = "linux")]
pub mod linux {
    pub mod input_surface;
}

#[cfg(target_os = "windows")]
pub mod windows {
    pub mod input_surface;
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub mod linux {
    pub mod input_surface;
}
