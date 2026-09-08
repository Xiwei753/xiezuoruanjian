#!/usr/bin/env python3
"""Contract tests for Android GitHub Actions workflow.

Verifies invariants that must hold for the Android CI:
- Gradle cache is configured (gradle/actions/setup-gradle present)
- cargo-ndk version is pinned via CARGO_NDK_VERSION env var
- Rust cache has shared-key for registry/git sharing across matrix jobs
- Rust cache key isolates target by flavor (固定 arm64-v8a)
- Build matrix only contains no-ai/ai flavor with fixed arm64-v8a ABI
- No test steps removed
- cargo-ndk is not installed via bare `cargo install cargo-ndk` (must be pinned)
- Build job actually builds APK (no --skip-gradle)
- APK artifacts exist for both no-ai and ai flavors (arm64-v8a)
- No emulator artifacts or steps in workflow
"""

import sys
from pathlib import Path

import yaml


WORKFLOW_PATH = Path(".github/workflows/android_debug_build.yml")


def load_workflow():
    text = WORKFLOW_PATH.read_text(encoding="utf-8")
    return yaml.safe_load(text), text


def test_gradle_cache_configured(wf, _text):
    """Verify Gradle cache is configured for all jobs except Rust core test jobs."""
    for job_name, job in wf.get("jobs", {}).items():
        if job_name.startswith("core-"):
            continue
        steps = job.get("steps", [])
        has_setup_gradle = any(
            "gradle/actions/setup-gradle" in str(s.get("uses", ""))
            for s in steps
        )
        assert has_setup_gradle, (
            f"Job '{job_name}' missing gradle/actions/setup-gradle step"
        )


def test_cargo_ndk_version_pinned(wf, _text):
    """Verify cargo-ndk version is pinned via CARGO_NDK_VERSION env var."""
    env = wf.get("env", {})
    assert "CARGO_NDK_VERSION" in env, "CARGO_NDK_VERSION env var missing"
    version = env["CARGO_NDK_VERSION"]
    assert version and version != "latest", (
        f"CARGO_NDK_VERSION must be pinned, got: {version!r}"
    )


def test_cargo_ndk_install_uses_pinned_version(wf, _text):
    """Verify cargo-ndk installation uses --version flag with pinned version."""
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
    """Verify no bare 'cargo install cargo-ndk' without version pinning."""
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
    """Verify Rust cache in build job has shared-key for registry/git sharing."""
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


def test_rust_cache_key_includes_arm64_v8a(wf, _text):
    """Verify Rust cache key includes arm64-v8a for target isolation."""
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
            assert "arm64-v8a" in key, (
                "Rust cache key must include fixed arm64-v8a for target isolation"
            )
            assert "matrix.abi" not in key, (
                "Rust cache key must not reference matrix.abi (abi matrix removed)"
            )


def test_build_matrix_only_flavor(wf, _text):
    """Verify build matrix only contains flavor key (no-ai/ai) with fixed arm64-v8a."""
    build_job = wf["jobs"]["build"]
    matrix = build_job.get("strategy", {}).get("matrix", {})
    assert "flavor" in matrix, "Build matrix must have flavor key"
    flavors = matrix["flavor"]
    assert flavors == ["no-ai", "ai"], (
        f"Build matrix flavor must be [no-ai, ai], got: {flavors}"
    )
    assert "abi" not in matrix, (
        "Build matrix must not have abi key (abi matrix removed; only arm64-v8a)"
    )
    includes = matrix.get("include", [])
    assert not includes or not any(i.get("abi") == "universal" for i in includes), (
        "Build matrix include must not contain universal ABI"
    )


def test_build_job_builds_apk_not_skip_gradle(wf, text):
    """Verify build job does not skip Gradle APK build (--skip-gradle not present)."""
    assert "--skip-gradle" not in text, (
        "Workflow must not contain --skip-gradle; build job must actually build APK"
    )


def test_apk_artifacts_exist(wf, _text):
    """Verify APK artifacts exist for both no-ai and ai flavors (arm64-v8a)."""
    build_job = wf["jobs"]["build"]
    steps = build_job.get("steps", [])
    
    apk_artifacts = []
    for s in steps:
        uses = s.get("uses", "")
        if "upload-artifact" in uses:
            name = s.get("with", {}).get("name", "")
            path = s.get("with", {}).get("path", "")
            # Check for APK artifact: name contains sujian-android and arm64-v8a, or path contains APK
            if "sujian-android-" in name and "arm64-v8a" in name:
                apk_artifacts.append(name)
            # Also check path if it contains APK output
            elif ".apk" in path and "apk" in name.lower():
                apk_artifacts.append(name)
    
    # Check if we have a single template-based artifact (using matrix.flavor)
    # This is the case when name contains ${{ matrix.flavor }}
    has_template_artifact = any("matrix.flavor" in name or "${{" in name for name in apk_artifacts)
    
    if has_template_artifact:
        # Template-based artifact is valid - it will produce both no-ai and ai at runtime
        assert len(apk_artifacts) == 1, (
            f"Expected exactly 1 template-based APK artifact, found {len(apk_artifacts)}: {apk_artifacts}"
        )
        return
    
    # Static artifact names: verify both no-ai and ai exist
    assert len(apk_artifacts) >= 2, (
        f"Expected at least 2 APK artifacts (no-ai/ai), found {len(apk_artifacts)}: {apk_artifacts}"
    )
    
    has_no_ai = any("no-ai" in name or "noAi" in name for name in apk_artifacts)
    has_ai = any("-ai-" in name or "-ai-debug" in name for name in apk_artifacts)
    
    assert has_no_ai, f"Missing no-ai APK artifact. Found: {apk_artifacts}"
    assert has_ai, f"Missing ai APK artifact. Found: {apk_artifacts}"


def test_workflow_no_emulator_artifacts(wf, text):
    """Verify workflow does not contain any emulator-related steps or artifacts."""
    forbidden = [
        "reactivecircus/android-emulator-runner",
        "Enable KVM group permissions",
        "connected${FLAVOR_CAP}DebugAndroidTest",
        "install${FLAVOR_CAP}DebugAndroidTest",
    ]
    
    for pattern in forbidden:
        assert pattern not in text, (
            f"Workflow must not contain emulator-related pattern '{pattern}'"
        )


def test_rust_test_has_abi_guard(wf, _text):
    """Verify Rust tests are in separate jobs, not in build matrix."""
    # Build job should not contain any cargo test steps
    build_job = wf["jobs"]["build"]
    steps = build_job.get("steps", [])
    rust_test_steps = [
        s for s in steps if "cargo test" in s.get("run", "")
    ]
    assert len(rust_test_steps) == 0, (
        f"Build matrix must not contain Rust test steps, found {len(rust_test_steps)}"
    )
    
    # Verify core-common-test and core-ai-test jobs exist and have correct tests
    common_job = wf["jobs"]["core-common-test"]["steps"]
    ai_job = wf["jobs"]["core-ai-test"]["steps"]
    common_steps = [s for s in common_job if "cargo test" in s.get("run", "")]
    ai_steps = [s for s in ai_job if "cargo test" in s.get("run", "")]
    
    assert len(common_steps) == 1, f"Expected exactly 1 common Rust test step, found {len(common_steps)}"
    assert "cargo test -p writer_core" in common_steps[0]["run"], (
        "Core common test must run `cargo test -p writer_core`"
    )
    assert len(ai_steps) == 1, f"Expected exactly 1 AI Rust test step, found {len(ai_steps)}"
    assert "--features ai" in ai_steps[0]["run"] and "--test ai_feature" in ai_steps[0]["run"], (
        "Core AI test must run only the ai_feature target with the ai feature"
    )


def test_jvm_unit_test_has_abi_guard(wf, _text):
    """Verify JVM unit tests are in separate jobs, not in build matrix."""
    # Build job should not contain any gradle test steps
    build_job = wf["jobs"]["build"]
    steps = build_job.get("steps", [])
    jvm_test_steps = [
        s for s in steps
        if "gradlew" in s.get("run", "") and "UnitTest" in s.get("run", "")
    ]
    assert len(jvm_test_steps) == 0, (
        f"Build matrix must not contain JVM unit test steps, found {len(jvm_test_steps)}"
    )
    
    # Verify android-unit-test and android-ai-test jobs exist and have correct tests
    common_job = wf["jobs"]["android-unit-test"]["steps"]
    ai_job = wf["jobs"]["android-ai-test"]["steps"]
    common_steps = [s for s in common_job if "gradlew" in s.get("run", "") and "UnitTest" in s.get("run", "")]
    ai_steps = [s for s in ai_job if "gradlew" in s.get("run", "") and "UnitTest" in s.get("run", "")]
    
    assert len(common_steps) == 1, f"Expected exactly 1 common JVM test step, found {len(common_steps)}"
    assert "testNoAiDebugUnitTest" in common_steps[0]["run"], (
        "Android common unit test must run testNoAiDebugUnitTest"
    )
    assert len(ai_steps) == 1, f"Expected exactly 1 AI JVM test step, found {len(ai_steps)}"
    assert "testAiDebugUnitTest" in ai_steps[0]["run"], (
        "Android AI unit test must run testAiDebugUnitTest"
    )


def test_rust_and_jvm_test_execute_once_per_flavor(wf, _text):
    """Verify Rust and JVM tests execute exactly once per flavor in separate jobs."""
    # Build job should not contain any test steps
    build_job = wf["jobs"]["build"]
    steps = build_job.get("steps", [])
    rust_test_steps = [s for s in steps if "cargo test" in s.get("run", "")]
    assert len(rust_test_steps) == 0, (
        f"Build job must not contain Rust test steps, found {len(rust_test_steps)}"
    )
    jvm_test_steps = [
        s for s in steps
        if "gradlew" in s.get("run", "") and "UnitTest" in s.get("run", "")
    ]
    assert len(jvm_test_steps) == 0, (
        f"Build job must not contain JVM unit test steps, found {len(jvm_test_steps)}"
    )
    
    # Verify each independent test job has exactly one test step
    common_rust = [s for s in wf["jobs"]["core-common-test"]["steps"] if "cargo test" in s.get("run", "")]
    ai_rust = [s for s in wf["jobs"]["core-ai-test"]["steps"] if "cargo test" in s.get("run", "")]
    assert len(common_rust) == 1, (
        f"Expected exactly 1 common Rust test step, found {len(common_rust)}"
    )
    assert len(ai_rust) == 1, (
        f"Expected exactly 1 AI Rust test step, found {len(ai_rust)}"
    )
    
    common_jvm = [
        s for s in wf["jobs"]["android-unit-test"]["steps"]
        if "gradlew" in s.get("run", "") and "UnitTest" in s.get("run", "")
    ]
    ai_jvm = [
        s for s in wf["jobs"]["android-ai-test"]["steps"]
        if "gradlew" in s.get("run", "") and "UnitTest" in s.get("run", "")
    ]
    assert len(common_jvm) == 1, (
        f"Expected exactly 1 common JVM test step, found {len(common_jvm)}"
    )
    assert len(ai_jvm) == 1, (
        f"Expected exactly 1 AI JVM test step, found {len(ai_jvm)}"
    )


def main():
    wf, text = load_workflow()
    tests = [
        test_gradle_cache_configured,
        test_cargo_ndk_version_pinned,
        test_cargo_ndk_install_uses_pinned_version,
        test_no_bare_cargo_install_cargo_ndk,
        test_rust_cache_has_shared_key,
        test_rust_cache_key_includes_arm64_v8a,
        test_build_matrix_only_flavor,
        test_build_job_builds_apk_not_skip_gradle,
        test_apk_artifacts_exist,
        test_workflow_no_emulator_artifacts,
        test_rust_test_has_abi_guard,
        test_jvm_unit_test_has_abi_guard,
        test_rust_and_jvm_test_execute_once_per_flavor,
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
