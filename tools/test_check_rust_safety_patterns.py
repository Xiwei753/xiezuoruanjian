#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("check_rust_safety_patterns.py")
SPEC = importlib.util.spec_from_file_location("check_rust_safety_patterns", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class RustSafetyPatternTests(unittest.TestCase):
    def rule_names(self, source: str) -> set[str]:
        return {
            finding.rule
            for finding in MODULE.scan_text(Path("sample.rs"), source)
        }

    def test_detects_known_compiler_bypass_patterns(self) -> None:
        source = r'''
#![allow(non_snake_case, deprecated)]
#[allow(non_snake_case, dead_code)]
struct Adapter(*mut std::ffi::c_void);
unsafe impl Send for Adapter {}
unsafe impl Sync for Adapter {}

fn aliases(app: &AppBackend) {
    let ptr = app as *const AppBackend as *mut AppBackend;
    let _borrow = unsafe { &mut *app };
    let _ = Box::leak(Box::new(String::new()));
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {}));
    if expected_revision == 0 {}
}
'''
        rules = self.rule_names(source)
        self.assertIn("unsafe-impl-send-sync", rules)
        self.assertIn("intentional-resource-leak", rules)
        self.assertIn("crate-wide-allow", rules)
        self.assertIn("broad-dead-code-allow", rules)
        self.assertIn("app-backend-mut-pointer-cast", rules)
        self.assertIn("app-backend-mut-alias", rules)
        self.assertIn("revision-zero-bypass", rules)
        self.assertIn("assert-unwind-safe", rules)
        self.assertIn("undocumented-unsafe-block", rules)

    def test_accepts_documented_ffi_unsafe_boundary(self) -> None:
        source = r'''
fn read_c_string(ptr: *const std::ffi::c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: ptr was checked for null and the C ABI requires a live NUL-terminated string.
    unsafe { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}
'''
        self.assertEqual(self.rule_names(source), set())

    def test_ignores_comments_and_string_literals(self) -> None:
        source = r'''
// unsafe impl Send for Fake {}
const MESSAGE: &str = "Box::leak and expected_revision == 0";
const RAW: &str = r#"#[allow(dead_code)] and AssertUnwindSafe"#;
'''
        self.assertEqual(self.rule_names(source), set())

    def test_repository_scan_skips_generated_and_build_directories(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "generated").mkdir()
            (root / "target").mkdir()
            (root / "src" / "safe.rs").write_text("fn ok() {}\n", encoding="utf-8")
            (root / "generated" / "bad.rs").write_text(
                "unsafe impl Send for Fake {}\n", encoding="utf-8"
            )
            (root / "target" / "bad.rs").write_text(
                "unsafe impl Sync for Fake {}\n", encoding="utf-8"
            )
            self.assertEqual(MODULE.scan_repository(root), [])

    def test_accepts_justified_dead_code_allow(self) -> None:
        source = r'''
#[allow(dead_code)] // qmetaobject macro field used by Qt meta-object system
struct Foo { x: i32 }
'''
        rules = self.rule_names(source)
        self.assertNotIn("broad-dead-code-allow", rules)

    def test_accepts_justified_assert_unwind_safe(self) -> None:
        source = r'''
fn ffi_boundary() {
    // SAFETY: AssertUnwindSafe needed for FFI boundary catch_unwind; closure only accesses owned data.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {}));
}
'''
        rules = self.rule_names(source)
        self.assertNotIn("assert-unwind-safe", rules)

    def test_accepts_justified_app_backend_mut_alias(self) -> None:
        source = r'''
fn with_app_mut(&mut self) {
    // SAFETY: pointer from QObjectBox-pinned AppBackend; null-guarded; single-threaded (Rc).
    unsafe { &mut *app }
}
'''
        rules = self.rule_names(source)
        self.assertNotIn("app-backend-mut-alias", rules)

    def test_flags_unjustified_patterns(self) -> None:
        source = r'''
#[allow(dead_code)]
struct Unjustified { x: i32 }
fn bad() {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {}));
    unsafe { &mut *app }
}
'''
        rules = self.rule_names(source)
        self.assertIn("broad-dead-code-allow", rules)
        self.assertIn("assert-unwind-safe", rules)
        self.assertIn("app-backend-mut-alias", rules)

    def test_detects_transmute_pointer_escape(self) -> None:
        source = r'''
fn bad() {
    let ptr: *const RefCell<AppBackend> = std::mem::transmute(pinned);
}
'''
        rules = self.rule_names(source)
        self.assertIn("transmute-pointer-escape", rules)

    def test_accepts_justified_transmute(self) -> None:
        source = r'''
fn qt_interop() {
    // SAFETY: QObjectPinned is #[repr(transparent)] over &RefCell<T>; pointer cast extracts inner reference.
    let ptr: *const RefCell<AppBackend> = std::mem::transmute(pinned);
}
'''
        rules = self.rule_names(source)
        self.assertNotIn("transmute-pointer-escape", rules)

    def test_detects_production_lock_unwrap(self) -> None:
        source = r'''
fn production_code(mutex: &Mutex<Vec<u8>>) {
    let guard = mutex.lock().unwrap();
}
'''
        rules = self.rule_names(source)
        self.assertIn("production-lock-unwrap", rules)

    def test_accepts_test_lock_unwrap(self) -> None:
        source = r'''
#[cfg(test)]
mod tests {
    fn test_something(mutex: &Mutex<Vec<u8>>) {
        let guard = mutex.lock().unwrap();
    }
}
'''
        rules = self.rule_names(source)
        self.assertNotIn("production-lock-unwrap", rules)

    def test_accepts_justified_lock_unwrap(self) -> None:
        source = r'''
fn init_once(mutex: &Mutex<()>) {
    // SAFETY: lock only held during init; no other thread can poison it.
    let guard = mutex.lock().unwrap();
}
'''
        rules = self.rule_names(source)
        self.assertNotIn("production-lock-unwrap", rules)

    def test_detects_safe_app_ptr_usage(self) -> None:
        source = r'''
fn bad() {
    let ptr = SafeAppPtr::new();
}
'''
        rules = self.rule_names(source)
        self.assertIn("safe-app-ptr-usage", rules)

    def test_detects_rc_cell_raw_pointer(self) -> None:
        source = r'''
struct Bad {
    cell: Rc<Cell<*const RefCell<AppBackend>>>,
}
'''
        rules = self.rule_names(source)
        self.assertIn("rc-cell-raw-pointer", rules)

    def test_accepts_rc_refcell_pattern(self) -> None:
        source = r'''
struct Good {
    inner: Rc<RefCell<AppBackend>>,
}
'''
        rules = self.rule_names(source)
        self.assertNotIn("rc-cell-raw-pointer", rules)


if __name__ == "__main__":
    unittest.main()
