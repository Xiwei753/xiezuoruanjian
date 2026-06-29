#!/usr/bin/env python3
"""Tests for check_doc_links.py legacy context whitelist logic.

Covers:
  - AGENTS.md: "document_handler.rs 已删除（legacy），不得恢复" → PASS
  - docs/foo.md: "请修改 SmoothCursor.qml" → FAIL
  - docs/foo.md: "SmoothCursor.qml 已删除，不得恢复" → PASS
  - docs/foo.md: "document_handler.rs is deprecated" → PASS
  - docs/foo.md: "apps/desktop/src/document_handler.rs is deleted" → PASS
  - docs/foo.md: "EditorPage.qml is legacy, must not restore" → PASS
  - docs/foo.md: "Use EditorPage.qml for editing" → FAIL
  - AGENTS.md: "EditorPage.qml" (no keyword on same line) → PASS (AGENTS.md always passes)
  - docs/foo.md: "apps/desktop/src/document_handler.rs" (no keyword) → FAIL
"""
import os
import re
import sys
import tempfile
import shutil

# Import the module under test
sys.path.insert(0, os.path.dirname(__file__))
import check_doc_links as cdl


def test_is_legacy_context():
    """Test the is_legacy_context helper function."""
    # Condition A: AGENTS.md always passes
    assert cdl.is_legacy_context('AGENTS.md', 'some random line') is True
    assert cdl.is_legacy_context('path/to/AGENTS.md', 'some random line') is True

    # Condition B: keywords in line
    assert cdl.is_legacy_context('docs/foo.md', 'document_handler.rs 已删除（legacy），不得恢复') is True
    assert cdl.is_legacy_context('docs/foo.md', 'This is deleted') is True
    assert cdl.is_legacy_context('docs/foo.md', 'This is legacy') is True
    assert cdl.is_legacy_context('docs/foo.md', 'This is deprecated') is True
    assert cdl.is_legacy_context('docs/foo.md', 'must not restore this') is True
    assert cdl.is_legacy_context('docs/foo.md', 'This is 废弃') is True
    assert cdl.is_legacy_context('docs/foo.md', 'This is DELETED') is True  # case-insensitive
    assert cdl.is_legacy_context('docs/foo.md', 'This is LEGACY') is True  # case-insensitive

    # No keyword, not AGENTS.md
    assert cdl.is_legacy_context('docs/foo.md', '请修改 SmoothCursor.qml') is False
    assert cdl.is_legacy_context('docs/foo.md', 'Use EditorPage.qml for editing') is False
    assert cdl.is_legacy_context('README.md', 'document_handler.rs is important') is False


def _run_check_in_tmpdir(md_files):
    """Create a temp repo with given markdown files and run check_links.

    Returns (broken_count, output_lines).
    """
    tmpdir = tempfile.mkdtemp(prefix='test_doc_links_')
    try:
        # Create a minimal repo structure
        os.makedirs(os.path.join(tmpdir, 'docs'), exist_ok=True)
        os.makedirs(os.path.join(tmpdir, 'apps', 'desktop', 'src'), exist_ok=True)
        os.makedirs(os.path.join(tmpdir, 'core', 'writer_core', 'src'), exist_ok=True)

        # Create some real files so path checks don't false-positive
        with open(os.path.join(tmpdir, 'core', 'writer_core', 'src', 'facade.rs'), 'w', encoding='utf-8') as f:
            f.write('// real file\n')
        with open(os.path.join(tmpdir, 'Cargo.toml'), 'w', encoding='utf-8') as f:
            f.write('[workspace]\n')

        # Create the markdown files
        for rel_path, content in md_files.items():
            full_path = os.path.join(tmpdir, rel_path)
            os.makedirs(os.path.dirname(full_path), exist_ok=True)
            with open(full_path, 'w', encoding='utf-8') as f:
                f.write(content)

        # Monkey-patch repo_root and run
        original_check = cdl.check_links
        broken_count = 0
        output_lines = []

        # We'll capture output by redirecting stdout
        import io
        old_stdout = sys.stdout
        sys.stdout = io.StringIO()

        # Temporarily override the repo root calculation
        original_dirname = os.path.dirname
        def patched_dirname(path):
            if path.endswith('check_doc_links.py'):
                return os.path.join(tmpdir, 'tools')
            return original_dirname(path)

        try:
            # Patch
            import check_doc_links
            old_dirname = check_doc_links.os.path.dirname
            check_doc_links.os.path.dirname = patched_dirname

            # Also need to make the repo root resolve to tmpdir
            # The script does: repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
            # So if __file__ is tmpdir/tools/check_doc_links.py, repo_root = tmpdir
            # We'll just call check_links and catch SystemExit
            try:
                check_doc_links.check_links()
            except SystemExit as e:
                pass

            output = sys.stdout.getvalue()
            output_lines = output.strip().split('\n')
            broken_count = output.count('Broken ')
        finally:
            check_doc_links.os.path.dirname = old_dirname
            sys.stdout = old_stdout

        return broken_count, output_lines

    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def test_agents_md_legacy_pass():
    """AGENTS.md: 'document_handler.rs 已删除（legacy），不得恢复' → PASS"""
    broken, _ = _run_check_in_tmpdir({
        'AGENTS.md': '# Rules\n| document_handler.rs 已删除（legacy），不得恢复 |\n',
    })
    # Should not report document_handler.rs as broken (AGENTS.md context)
    # Note: there may be other broken references in the test content, but
    # document_handler.rs should NOT be among them
    assert broken == 0, f"Expected 0 broken references, got {broken}"


def test_docs_plain_reference_fail():
    """docs/foo.md: '请修改 SmoothCursor.qml' → FAIL"""
    broken, output = _run_check_in_tmpdir({
        'docs/foo.md': '# Guide\n请修改 SmoothCursor.qml\n',
    })
    assert broken >= 1, f"Expected >= 1 broken reference, got {broken}"
    found = any('SmoothCursor.qml' in line for line in output)
    assert found, "Expected SmoothCursor.qml to be reported as broken"


def test_docs_legacy_keyword_pass():
    """docs/foo.md: 'SmoothCursor.qml 已删除，不得恢复' → PASS"""
    broken, output = _run_check_in_tmpdir({
        'docs/foo.md': '# Guide\nSmoothCursor.qml 已删除，不得恢复\n',
    })
    # SmoothCursor.qml should NOT be reported as broken
    found = any('SmoothCursor.qml' in line for line in output)
    assert not found, "SmoothCursor.qml should NOT be reported as broken when legacy keyword present"


def test_docs_deprecated_keyword_pass():
    """docs/foo.md: 'document_handler.rs is deprecated' → PASS"""
    broken, output = _run_check_in_tmpdir({
        'docs/foo.md': '# Guide\ndocument_handler.rs is deprecated\n',
    })
    found = any('document_handler.qml' in line for line in output)
    assert not found, "document_handler.rs should NOT be reported as broken when deprecated keyword present"


def test_docs_legacy_path_with_keyword_pass():
    """docs/foo.md: 'apps/desktop/src/document_handler.rs is deleted' → PASS"""
    broken, output = _run_check_in_tmpdir({
        'docs/foo.md': '# Guide\napps/desktop/src/document_handler.rs is deleted\n',
    })
    found = any('document_handler.rs' in line for line in output)
    assert not found, "Legacy path should NOT be reported when deleted keyword present"


def test_docs_editor_page_legacy_pass():
    """docs/foo.md: 'EditorPage.qml is legacy, must not restore' → PASS"""
    broken, output = _run_check_in_tmpdir({
        'docs/foo.md': '# Guide\nEditorPage.qml is legacy, must not restore\n',
    })
    found = any('EditorPage.qml' in line for line in output)
    assert not found, "EditorPage.qml should NOT be reported as broken when legacy keyword present"


def test_docs_editor_page_plain_fail():
    """docs/foo.md: 'Use EditorPage.qml for editing' → FAIL"""
    broken, output = _run_check_in_tmpdir({
        'docs/foo.md': '# Guide\nUse EditorPage.qml for editing\n',
    })
    assert broken >= 1, f"Expected >= 1 broken reference, got {broken}"
    found = any('EditorPage.qml' in line for line in output)
    assert found, "EditorPage.qml should be reported as broken without legacy keyword"


def test_agents_md_always_pass():
    """AGENTS.md: 'EditorPage.qml' (no keyword on same line) → PASS (AGENTS.md always passes)"""
    broken, output = _run_check_in_tmpdir({
        'AGENTS.md': '# Rules\n| EditorPage.qml |\n',
    })
    found = any('EditorPage.qml' in line for line in output)
    assert not found, "EditorPage.qml in AGENTS.md should NOT be reported as broken"


def test_docs_legacy_path_no_keyword_fail():
    """docs/foo.md: 'apps/desktop/src/document_handler.rs' (no keyword) → FAIL"""
    broken, output = _run_check_in_tmpdir({
        'docs/foo.md': '# Guide\nSee apps/desktop/src/document_handler.rs for details\n',
    })
    assert broken >= 1, f"Expected >= 1 broken reference, got {broken}"
    found = any('document_handler.rs' in line for line in output)
    assert found, "Legacy path without keyword should be reported as broken"


# ── Forbidden phrase tests ──

def test_forbidden_phrase_in_normal_doc_fails():
    """Normal doc with forbidden phrase should be reported"""
    broken, output = _run_check_in_tmpdir({
        'docs/guide.md': '# Guide\nUse DocumentHandler for editing\n',
    })
    # Should report a forbidden phrase (check for the error line, not the summary)
    found = any('Forbidden phrase in ' in line for line in output)
    assert found, "DocumentHandler in normal doc should be reported as forbidden phrase"


def test_forbidden_phrase_in_agents_md_passes():
    """AGENTS.md with forbidden phrase should pass (AGENTS.md is always allowed)"""
    broken, output = _run_check_in_tmpdir({
        'AGENTS.md': '# Rules\n| DocumentHandler | 已删除（legacy），不得恢复 |\n',
    })
    # AGENTS.md is always allowed for forbidden phrases (check for the error line, not the summary)
    found = any('Forbidden phrase in ' in line for line in output)
    assert not found, "Forbidden phrase in AGENTS.md should NOT be reported"


def test_forbidden_phrase_with_legacy_keyword_passes():
    """Doc with forbidden phrase + legacy keyword on same line should pass"""
    broken, output = _run_check_in_tmpdir({
        'docs/guide.md': '# Guide\nDocumentHandler is deprecated and deleted\n',
    })
    # Legacy keyword on same line should allow the forbidden phrase (check for the error line, not the summary)
    found = any('Forbidden phrase in ' in line for line in output)
    assert not found, "Forbidden phrase with legacy keyword should NOT be reported"


def test_forbidden_code_pattern_in_qml_fails():
    """QML file with DocumentHandler should be reported"""
    tmpdir = tempfile.mkdtemp(prefix='test_doc_links_')
    try:
        # Create a minimal repo structure
        os.makedirs(os.path.join(tmpdir, 'docs'), exist_ok=True)
        os.makedirs(os.path.join(tmpdir, 'apps', 'desktop', 'qml'), exist_ok=True)

        # Create a real QML file with forbidden pattern
        with open(os.path.join(tmpdir, 'apps', 'desktop', 'qml', 'Test.qml'), 'w', encoding='utf-8') as f:
            f.write('import QtQuick 2.0\nDocumentHandler {\n}\n')

        # Create a minimal doc file so the scan runs
        with open(os.path.join(tmpdir, 'docs', 'guide.md'), 'w', encoding='utf-8') as f:
            f.write('# Guide\n')

        import io
        old_stdout = sys.stdout
        sys.stdout = io.StringIO()

        original_dirname = os.path.dirname
        def patched_dirname(path):
            if path.endswith('check_doc_links.py'):
                return os.path.join(tmpdir, 'tools')
            return original_dirname(path)

        try:
            import check_doc_links
            old_dirname = check_doc_links.os.path.dirname
            check_doc_links.os.path.dirname = patched_dirname

            try:
                check_doc_links.check_links()
            except SystemExit:
                pass

            output = sys.stdout.getvalue()
            output_lines = output.strip().split('\n')
        finally:
            check_doc_links.os.path.dirname = old_dirname
            sys.stdout = old_stdout

        found = any('Forbidden code pattern in ' in line for line in output_lines)
        assert found, "DocumentHandler in QML file should be reported as forbidden code pattern"

    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)


def main():
    tests = [
        test_is_legacy_context,
        test_agents_md_legacy_pass,
        test_docs_plain_reference_fail,
        test_docs_legacy_keyword_pass,
        test_docs_deprecated_keyword_pass,
        test_docs_legacy_path_with_keyword_pass,
        test_docs_editor_page_legacy_pass,
        test_docs_editor_page_plain_fail,
        test_agents_md_always_pass,
        test_docs_legacy_path_no_keyword_fail,
        test_forbidden_phrase_in_normal_doc_fails,
        test_forbidden_phrase_in_agents_md_passes,
        test_forbidden_phrase_with_legacy_keyword_passes,
        test_forbidden_code_pattern_in_qml_fails,
    ]

    passed = 0
    failed = 0
    for test in tests:
        name = test.__doc__ or test.__name__
        try:
            test()
            print(f"  PASS: {name}")
            passed += 1
        except AssertionError as e:
            print(f"  FAIL: {name}")
            print(f"        {e}")
            failed += 1
        except Exception as e:
            print(f"  ERROR: {name}")
            print(f"        {type(e).__name__}: {e}")
            failed += 1

    print(f"\n{passed} passed, {failed} failed out of {len(tests)} tests")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
