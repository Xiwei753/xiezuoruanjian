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
from pathlib import Path, PurePosixPath

import yaml


WORKFLOW_PATH = Path(".github/workflows/android_debug_build.yml")


def load_workflow():
    text = WORKFLOW_PATH.read_text(encoding="utf-8")
    return yaml.safe_load(text), text


def test_gradle_cache_configured(wf, _text):
    # #597：Rust 专属 job（core-common-test/core-ai-test）不运行 Gradle，
    # 不需要 setup-gradle；其余 job 必须配置。
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


def test_emulator_test_excludes_native_build_task(wf, _text):
    emulator_job = wf["jobs"]["emulator-test"]
    steps = emulator_job.get("steps", [])
    for s in steps:
        # The Gradle command is inside the 'script' key of android-emulator-runner
        script = s.get("with", {}).get("script", "")
        run_cmd = s.get("run", "")
        combined = script or run_cmd
        if "gradlew" in combined and "connected" in combined:
            assert "-x" in combined, (
                "emulator-test Gradle command must exclude buildWriterNative task "
                "(-x build<Variant>WriterNative) because native artifacts are "
                "restored from the build job's upload, not rebuilt."
            )
            assert "WriterNative" in combined, (
                "emulator-test must explicitly exclude the WriterNative build task "
                "since cargo-ndk/Rust toolchain are not installed in this job."
            )
            return
    raise AssertionError("Could not find gradlew connected* command in emulator-test")


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


def _get_upload_paths_for_native_artifact(wf):
    """Return list of upload paths for the native-<flavor>-x86_64 artifact, or None."""
    build_job = wf["jobs"]["build"]
    steps = build_job.get("steps", [])
    for s in steps:
        uses = s.get("uses", "")
        with_ = s.get("with", {})
        name = with_.get("name", "")
        if "upload-artifact" in uses and name.startswith("native-") and name.endswith("-x86_64"):
            raw = with_.get("path", "")
            return [p.strip() for p in raw.strip().splitlines() if p.strip()]
    return None


def _get_download_path_for_native_artifact(wf):
    """Return download path for native-<flavor>-x86_64 artifact, or None."""
    emulator_job = wf["jobs"]["emulator-test"]
    steps = emulator_job.get("steps", [])
    for s in steps:
        uses = s.get("uses", "")
        with_ = s.get("with", {})
        name = with_.get("name", "")
        if "download-artifact" in uses and name.startswith("native-") and name.endswith("-x86_64"):
            return with_.get("path", "")
    return None


def _get_verify_file_paths(wf):
    """Return (so_path, uniffi_dir) from the verify step, or (None, None)."""
    emulator_job = wf["jobs"]["emulator-test"]
    steps = emulator_job.get("steps", [])
    for s in steps:
        name = s.get("name", "")
        run_cmd = s.get("run", "")
        if "verify" in name.lower() or "verify" in run_cmd.lower():
            so_path = None
            uniffi_dir = None
            for line in run_cmd.splitlines():
                line = line.strip()
                if 'SO_PATH="' in line:
                    so_path = line.split('"')[1]
                if line.startswith("UNIFFI_DIR=") and '="' in line:
                    uniffi_dir = line.split('"')[1]
            return so_path, uniffi_dir
    return None, None


def _common_path(paths):
    """Return the longest common ancestor directory of a list of POSIX paths."""
    if not paths:
        return ""
    parts_list = [PurePosixPath(p).parts for p in paths]
    common = []
    for i, part in enumerate(parts_list[0]):
        if all(len(parts) > i and parts[i] == part for parts in parts_list):
            common.append(part)
        else:
            break
    return str(PurePosixPath(*common)) if common else ""


def test_native_artifact_upload_paths_precise(wf, _text):
    paths = _get_upload_paths_for_native_artifact(wf)
    assert paths is not None, "native-no-ai-x86_64 upload step not found"
    assert len(paths) == 2, (
        f"Expected exactly 2 upload paths for native artifact, got {len(paths)}"
    )
    assert any("writer-native" in p for p in paths), (
        "Expected upload path containing writer-native/"
    )
    assert any("writer-uniffi" in p for p in paths), (
        "Expected upload path containing writer-uniffi/"
    )
    assert not any(p in ("apps/android/app/build", "apps/android/app/build/")
                    for p in paths), (
        "Upload path must not be the entire build/ directory; "
        "only generated/writer-native/ and generated/writer-uniffi/ are needed"
    )


def test_emulator_test_download_path_matches_upload_lca(wf, _text):
    upload_paths = _get_upload_paths_for_native_artifact(wf)
    assert upload_paths is not None, "native-no-ai-x86_64 upload step not found"
    download_path = _get_download_path_for_native_artifact(wf)
    assert download_path is not None, (
        "native-no-ai-x86_64 download step not found"
    )
    lca = _common_path(upload_paths)
    assert lca, f"Could not compute LCA of upload paths: {upload_paths}"
    expected = lca + "/"
    assert download_path == expected, (
        f"Download path mismatch: expected '{expected}' (LCA of upload paths), "
        f"got '{download_path}'. "
        "upload-artifact@v4 strips the LCA prefix from multi-path artifacts, "
        "so download must target the LCA to restore the expected directory layout."
    )


def test_rust_test_has_abi_guard(wf, _text):
    # #597：测试已移出构建矩阵 — 构建矩阵只负责编译/打包，不再内嵌任何测试步骤。
    build_job = wf["jobs"]["build"]
    steps = build_job.get("steps", [])
    rust_test_steps = [
        s for s in steps if "cargo test" in s.get("run", "")
    ]
    assert len(rust_test_steps) == 0, (
        f"Build matrix must not contain Rust test steps, found {len(rust_test_steps)}"
    )
    # 通用测试与 AI 专项测试分别落在独立 job，各恰好一次。
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
    # #597：Android JVM 测试同样在独立 job 中各跑一次，构建矩阵不内嵌测试。
    build_job = wf["jobs"]["build"]
    steps = build_job.get("steps", [])
    jvm_test_steps = [
        s for s in steps
        if "gradlew" in s.get("run", "") and "UnitTest" in s.get("run", "")
    ]
    assert len(jvm_test_steps) == 0, (
        f"Build matrix must not contain JVM unit test steps, found {len(jvm_test_steps)}"
    )
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
    build_job = wf["jobs"]["build"]
    matrix = build_job.get("strategy", {}).get("matrix", {})
    flavors = matrix.get("flavor", [])
    abis = matrix.get("abi", [])
    includes = matrix.get("include", [])
    all_combos = [
        {"flavor": f, "abi": a}
        for f in flavors
        for a in abis
    ] + includes
    steps = build_job.get("steps", [])
    for s in steps:
        run_cmd = s.get("run", "")
        condition = s.get("if", "")
        if "cargo test" in run_cmd and condition == "matrix.abi == 'arm64-v8a'":
            matching = [c for c in all_combos if c.get("abi") == "arm64-v8a"]
            for flavor in flavors:
                count = sum(1 for c in matching if c.get("flavor") == flavor)
                assert count == 1, (
                    f"Rust test should execute exactly once for flavor '{flavor}', "
                    f"but matrix expands to {count} matching combinations"
                )
        if "gradlew" in run_cmd and "UnitTest" in run_cmd and condition == "matrix.abi == 'arm64-v8a'":
            matching = [c for c in all_combos if c.get("abi") == "arm64-v8a"]
            for flavor in flavors:
                count = sum(1 for c in matching if c.get("flavor") == flavor)
                assert count == 1, (
                    f"JVM unit test should execute exactly once for flavor '{flavor}', "
                    f"but matrix expands to {count} matching combinations"
                )


def test_artifact_verify_paths_consistent_with_contract(wf, _text):
    upload_paths = _get_upload_paths_for_native_artifact(wf)
    assert upload_paths is not None, "native-no-ai-x86_64 upload step not found"
    download_path = _get_download_path_for_native_artifact(wf)
    assert download_path is not None, (
        "native-no-ai-x86_64 download step not found"
    )
    so_path, uniffi_dir = _get_verify_file_paths(wf)
    assert so_path, "Could not extract SO_PATH from verify step"
    assert uniffi_dir, "Could not extract UNIFFI_DIR from verify step"

    # After download, artifact root content (stripped LCA) is placed under download_path
    # So a verify path like "apps/android/app/build/generated/writer-native/..." must
    # be reachable as: <download_path>/<artifact_relative_path>
    lca = _common_path(upload_paths)
    # Artifact-relative path = verify path minus the LCA prefix
    for verify_path, label in [(so_path, "SO_PATH"), (uniffi_dir, "UNIFFI_DIR")]:
        if not verify_path.startswith(lca):
            raise AssertionError(
                f"{label} '{verify_path}' does not start with upload LCA '{lca}'; "
                f"download would not restore this path."
            )
        expected_prefix = download_path.rstrip("/")
        rel = verify_path[len(lca):].lstrip("/")
        expected_path = f"{expected_prefix}/{rel}"
        if not expected_path.startswith(expected_prefix.rstrip("/")):
            raise AssertionError(
                f"{label} relative path mismatch: "
                f"expected under '{expected_prefix}/', got '{expected_path}'"
            )


def test_ai_emulator_leg_fails_when_no_tests_match(wf, _text):
    # #597：AI 腿用官方包过滤只跑 AI 专项仪器测试，且必须 fail-on-no-matching。
    # 若 androidTestAi 源集被误删或过滤失效，connected 任务会静默通过 0 个测试，
    # 工作流必须在结果 XML 里硬核验 AiFlavorInstrumentationSmokeTest 真实执行。
    emulator_job = wf["jobs"]["emulator-test"]
    steps = emulator_job.get("steps", [])
    script = ""
    for s in steps:
        script = s.get("with", {}).get("script", "")
        if script:
            break
    assert script, "emulator-test job must have an android-emulator-runner script"
    assert "android.testInstrumentationRunnerArguments.package=com.xiwei.sujian.ai" in script, (
        "AI emulator leg must filter to the com.xiwei.sujian.ai package"
    )
    assert "AiFlavorInstrumentationSmokeTest" in script, (
        "AI emulator leg must verify AiFlavorInstrumentationSmokeTest results"
    )
    tail = script.split("AiFlavorInstrumentationSmokeTest", 1)[1]
    assert "exit 1" in tail, (
        "AI emulator leg must fail hard when no AI instrumented test matched"
    )


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
        test_emulator_test_excludes_native_build_task,
        test_emulator_matrix_not_reduced,
        test_native_artifact_upload_paths_precise,
        test_emulator_test_download_path_matches_upload_lca,
        test_artifact_verify_paths_consistent_with_contract,
        test_rust_test_has_abi_guard,
        test_jvm_unit_test_has_abi_guard,
        test_rust_and_jvm_test_execute_once_per_flavor,
        test_ai_emulator_leg_fails_when_no_tests_match,
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
