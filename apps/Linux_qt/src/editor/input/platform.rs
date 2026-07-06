//! Linux Qt input surface adapter selection.
//!
//! `qt_surface.rs` owns Qt event routing. Linux/fcitx5/ibus IME behavior lives
//! in the Linux adapter only; deferred pending-key state is not part of this crate.

pub mod linux {
    pub mod input_surface;
}
