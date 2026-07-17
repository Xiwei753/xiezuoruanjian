#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("check_rust_safety_patterns.py")
SPEC = importlib.util.spec_from_file_location("check_rust_safety_patterns", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RustSafetyPatternTests(unittest.TestCase):
    def rule_names(self, source: str) -> set[str]:
        return {
            finding.rule
            for finding in MODULE.scan_text(Path("sample.rs"), source)
        }

    def test_detects_known_compiler_bypass_patterns(self) -> None:
        source = r'''
#![allow(deprecated)]
#[allow(dead_code)]
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


if __name__ == "__main__":
    unittest.main()
