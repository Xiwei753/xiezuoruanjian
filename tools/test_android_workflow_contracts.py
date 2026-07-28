#!/usr/bin/env python3
"""Contract tests for Android GitHub Actions workflow.

Verifies invariants that must hold for the Android CI:
- Gradle cache is configured (gradle/actions/setup-gradle present)
- cargo-ndk version is pinned via CARGO_NDK_VERSION env var
- Cross-job artifact dependency: emulator-test depends on build and downloads native artifact
- Rust cache has shared-key for registry/git sharing across matrix jobs
- Rust cache key isolates target by flavor+abi
- Build matrix is not reduced (no missing flavors/abis)
- No test steps removed
- Native artifact upload exists for emulator reuse
- cargo-ndk is not installed via bare `cargo install cargo-ndk` (must be pinned)
"""

import sys
from pathlib import Path

import yaml


WORKFLOW_PATH = Path(".github/workflows/android_debug_build.yml")


def load_workflow():
    text = WORKFLOW_PATH.read_text(encoding="utf-8")
    return yaml.safe_load(text), text


def test_gradle_cache_configured(wf, _text):
    for job_name, job in wf.get("jobs", {}).items():
        steps = job.get("steps", [])
        has_setup_gradle = any(
            "gradle/actions/setup-gradle" in str(s.get("uses", ""))
            for s in steps
        )
        assert has_setup_gradle, (
            f"Job '{job_name}' missing gradle/actions/setup-gradle step"
        )


def test_cargo_ndk_version_pinned(wf, _text):
    env = wf.get("env", {})
    assert "CARGO_NDK_VERSION" in env, "CARGO_NDK_VERSION env var missing"
    version = env["CARGO_NDK_VERSION"]
    assert version and version != "latest", (
        f"CARGO_NDK_VERSION must be pinned, got: {version!r}"
    )


def test_cargo_ndk_install_uses_pinned_version(wf, _text):
    for job_name, job in wf.get("jobs", {}).items():
        steps = job.get("steps", [])
        for s in steps:
            run_cmd = s.get("run", "")
            if "cargo install cargo-ndk" in run_cmd:
                assert "--version" in run_cmd, (
                    f"Job '{job_name}': cargo install cargo-ndk must use --version flag"
                )
                assert "CARGO_NDK_VERSION" in run_cmd, (
                    f"Job '{job_name}': cargo install cargo-ndk must reference CARGO_NDK_VERSION"
                )


def test_no_bare_cargo_install_cargo_ndk(wf, _text):
    for job_name, job in wf.get("jobs", {}).items():
        steps = job.get("steps", [])
        for s in steps:
            run_cmd = s.get("run", "")
            if run_cmd.strip() == "cargo install cargo-ndk":
                raise AssertionError(
                    f"Job '{job_name}': bare 'cargo install cargo-ndk' forbidden; "
                    "must use --version ${{ env.CARGO_NDK_VERSION }} --locked"
                )


def test_rust_cache_has_shared_key(wf, _text):
    build_job = wf["jobs"]["build"]
    steps = build_job.get("steps", [])
    for s in steps:
        uses = s.get("uses", "")
        if "Swatinem/rust-cache" in uses:
            with_params = s.get("with", {})
            assert "shared-key" in with_params, (
                "Rust cache in build job must have shared-key for registry/git sharing"
            )
            assert with_params["shared-key"], "shared-key must not be empty"


def test_rust_cache_key_includes_flavor_abi(wf, _text):
    build_job = wf["jobs"]["build"]
    steps = build_job.get("steps", [])
    for s in steps:
        uses = s.get("uses", "")
        if "Swatinem/rust-cache" in uses:
            with_params = s.get("with", {})
            key = with_params.get("key", "")
            assert "flavor" in key or "matrix.flavor" in key, (
                "Rust cache key must include flavor for target isolation"
            )
            assert "abi" in key or "matrix.abi" in key, (
                "Rust cache key must include abi for target isolation"
            )


def test_emulator_test_depends_on_build(wf, _text):
    emulator_job = wf["jobs"].get("emulator-test", {})
    needs = emulator_job.get("needs", [])
    assert "build" in needs, "emulator-test must depend on build job"


def test_emulator_test_downloads_native_artifact(wf, _text):
    emulator_job = wf["jobs"]["emulator-test"]
    steps = emulator_job.get("steps", [])
    has_download = any(
        "download-artifact" in str(s.get("uses", ""))
        for s in steps
    )
    assert has_download, "emulator-test must download native build artifact"


def test_emulator_test_no_rust_toolchain(wf, _text):
    emulator_job = wf["jobs"]["emulator-test"]
    steps = emulator_job.get("steps", [])
    for s in steps:
        uses = s.get("uses", "")
        assert "dtolnay/rust-toolchain" not in uses, (
            "emulator-test should not install Rust toolchain; "
            "it reuses native artifacts from build job"
        )


def test_emulator_test_no_cargo_ndk(wf, _text):
    emulator_job = wf["jobs"]["emulator-test"]
    steps = emulator_job.get("steps", [])
    for s in steps:
        uses = s.get("uses", "")
        run_cmd = s.get("run", "")
        assert "cargo-ndk" not in uses and "cargo-ndk" not in run_cmd, (
            "emulator-test should not install/use cargo-ndk; "
            "it reuses native artifacts from build job"
        )


def test_build_matrix_not_reduced(wf, _text):
    build_job = wf["jobs"]["build"]
    matrix = build_job.get("strategy", {}).get("matrix", {})
    flavors = matrix.get("flavor", [])
    abis = matrix.get("abi", [])
    assert "no-ai" in flavors, "Build matrix must include no-ai flavor"
    assert "ai" in flavors, "Build matrix must include ai flavor"
    assert "arm64-v8a" in abis, "Build matrix must include arm64-v8a ABI"
    assert "x86_64" in abis, "Build matrix must include x86_64 ABI"
    includes = matrix.get("include", [])
    has_universal = any(i.get("abi") == "universal" for i in includes)
    assert has_universal, "Build matrix must include universal ABI"


def test_native_artifact_upload_exists(wf, _text):
    build_job = wf["jobs"]["build"]
    steps = build_job.get("steps", [])
    has_upload = any(
        "upload-artifact" in str(s.get("uses", ""))
        and "native" in str(s.get("with", {}).get("name", ""))
        for s in steps
    )
    assert has_upload, "Build job must upload native artifacts for emulator reuse"


def test_cargo_ndk_cached(wf, _text):
    build_job = wf["jobs"]["build"]
    steps = build_job.get("steps", [])
    has_cache = any(
        "actions/cache" in str(s.get("uses", ""))
        and "cargo-ndk" in str(s.get("with", {}).get("key", ""))
        for s in steps
    )
    assert has_cache, "Build job must cache cargo-ndk binary"


def test_emulator_test_verifies_artifacts(wf, _text):
    emulator_job = wf["jobs"]["emulator-test"]
    steps = emulator_job.get("steps", [])
    has_verify = any(
        "verify" in s.get("name", "").lower() or "verify" in s.get("run", "").lower()
        for s in steps
        if s.get("run")
    )
    assert has_verify, "emulator-test must verify downloaded native artifacts exist"


def test_emulator_matrix_not_reduced(wf, _text):
    emulator_job = wf["jobs"]["emulator-test"]
    matrix = emulator_job.get("strategy", {}).get("matrix", {})
    flavors = matrix.get("flavor", [])
    assert "no-ai" in flavors, "Emulator test matrix must include no-ai flavor"


def main():
    wf, text = load_workflow()
    tests = [
        test_gradle_cache_configured,
        test_cargo_ndk_version_pinned,
        test_cargo_ndk_install_uses_pinned_version,
        test_no_bare_cargo_install_cargo_ndk,
        test_rust_cache_has_shared_key,
        test_rust_cache_key_includes_flavor_abi,
        test_emulator_test_depends_on_build,
        test_emulator_test_downloads_native_artifact,
        test_emulator_test_no_rust_toolchain,
        test_emulator_test_no_cargo_ndk,
        test_build_matrix_not_reduced,
        test_native_artifact_upload_exists,
        test_cargo_ndk_cached,
        test_emulator_test_verifies_artifacts,
        test_emulator_matrix_not_reduced,
    ]
    failed = 0
    for t in tests:
        try:
            t(wf, text)
            print(f"  PASS  {t.__name__}")
        except AssertionError as e:
            print(f"  FAIL  {t.__name__}: {e}")
            failed += 1
        except Exception as e:
            print(f"  ERROR {t.__name__}: {e}")
            failed += 1
    print()
    if failed:
        print(f"{failed} test(s) failed.")
        sys.exit(1)
    else:
        print(f"All {len(tests)} contract tests passed.")


if __name__ == "__main__":
    main()
