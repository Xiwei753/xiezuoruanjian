#!/usr/bin/env python3
"""Linux -> HarmonyOS AGC invitation/open testing publisher for Sujian.

The script intentionally does not create/delete certificates, profiles, apps, groups,
or testers. It reuses the repository's existing HarmonyOS release signing setup and
an existing AGC test group. By default it also replaces old HarmonyOS test versions
before publishing the newest build:

  release .app -> inspect old test versions -> cancel/stop/delete when allowed
  -> create test version -> upload -> add package
  -> bind package/group -> optionally submit for review

Credentials are read by connect-api-cli from the current environment or repository
root .env. Secrets are never printed by this wrapper.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import zipfile
from pathlib import Path
from typing import Any, Iterable, Sequence

ROOT = Path(__file__).resolve().parents[1]
HARMONY_DIR = ROOT / "apps" / "harmony"
BUILD_PROFILE = HARMONY_DIR / "build-profile.json5"
DEFAULT_PACKAGE_NAME = "com.xiwei.sujian"
DEFAULT_CONNECT_API_CLI_NPM_VERSION = "1.1.3"


class PublishError(RuntimeError):
    pass


def eprint(*args: object) -> None:
    print(*args, file=sys.stderr)


def run(
    cmd: Sequence[str],
    *,
    cwd: Path = ROOT,
    env: dict[str, str] | None = None,
    capture: bool = False,
) -> subprocess.CompletedProcess[str]:
    eprint("+", " ".join(shlex.quote(str(part)) for part in cmd))
    proc = subprocess.run(
        [str(part) for part in cmd],
        cwd=cwd,
        env=env,
        text=True,
        stdout=subprocess.PIPE if capture else None,
        stderr=subprocess.PIPE if capture else None,
        check=False,
    )
    if proc.returncode != 0:
        if capture:
            _emit_safe_stderr(proc.stderr)
        raise PublishError(f"命令失败（exit={proc.returncode}）：{cmd[0]}")
    return proc


def _emit_safe_stderr(stderr: str | None) -> None:
    if not stderr:
        return
    # AGC CLI progress/error output does not normally contain credentials. Still
    # redact common secret/token forms before surfacing it.
    text = stderr
    patterns = (
        r"(?i)(access[_ -]?token\s*[:=]\s*)\S+",
        r"(?i)(authorization\s*[:=]\s*bearer\s+)\S+",
        r"(?i)(client[_ -]?secret\s*[:=]\s*)\S+",
        r"(?i)(storePassword\s*[:=]\s*)\S+",
        r"(?i)(keyPassword\s*[:=]\s*)\S+",
    )
    for pattern in patterns:
        text = re.sub(pattern, r"\1***", text)
    if text.strip():
        eprint(text.rstrip())


def parse_last_json(text: str) -> dict[str, Any]:
    """Return the last JSON object embedded in stdout.

    connect-api-cli <= 1.1.3 may mix non-JSON informational lines into stdout.
    Scanning for raw JSON objects makes the wrapper compatible without trusting the
    process exit code alone.
    """
    decoder = json.JSONDecoder()
    found: list[tuple[int, dict[str, Any]]] = []
    for index, char in enumerate(text):
        if char != "{":
            continue
        try:
            obj, consumed = decoder.raw_decode(text[index:])
        except json.JSONDecodeError:
            continue
        if isinstance(obj, dict):
            found.append((index + consumed, obj))
    if not found:
        raise PublishError("connect-api-cli 没有返回可解析的 JSON。")
    # Pick the object that ends last in stdout. This prefers the outer API
    # response over nested objects such as {"ret": ...}.
    return max(found, key=lambda item: item[0])[1]


def validate_business_result(data: dict[str, Any], action: str) -> None:
    ret = data.get("ret")
    if isinstance(ret, dict) and "code" in ret:
        code = ret.get("code")
        if str(code) != "0":
            raise PublishError(f"{action} 失败：ret.code={code}，msg={ret.get('msg', '')}")
        return

    # Some AppTest endpoints use rtnCode rather than ret.code.
    if "rtnCode" in data and str(data.get("rtnCode")) != "0":
        raise PublishError(
            f"{action} 失败：rtnCode={data.get('rtnCode')}，"
            f"rtnDesc={data.get('rtnDesc', '')}"
        )


def iter_dicts(value: Any) -> Iterable[dict[str, Any]]:
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from iter_dicts(child)
    elif isinstance(value, list):
        for child in value:
            yield from iter_dicts(child)


def first_value(data: dict[str, Any], keys: Sequence[str]) -> str | None:
    for obj in iter_dicts(data):
        for key in keys:
            value = obj.get(key)
            if isinstance(value, (str, int)) and str(value):
                return str(value)
    return None


def first_list_value(data: dict[str, Any], keys: Sequence[str]) -> str | None:
    for obj in iter_dicts(data):
        for key in keys:
            value = obj.get(key)
            if isinstance(value, list):
                for item in value:
                    if isinstance(item, (str, int)) and str(item):
                        return str(item)
    return None


def find_version_package_id(data: dict[str, Any], version_id: str) -> str | None:
    """Find the package ID already attached to one test version."""
    for obj in iter_dicts(data):
        value = obj.get("versionId")
        if value is None or str(value) != version_id:
            continue
        package_id = first_value(
            obj,
            ("pkgId", "packageId", "packageID", "pkgID"),
        )
        if package_id:
            return package_id
    return None


def normalize_test_window(
    start_time_ms: int | None,
    end_time_ms: int | None,
    *,
    now_ms: int | None = None,
) -> tuple[int, int]:
    """Return a valid testing window; default to now -> 30 days.

    Huawei currently caps one testing period at 90 days.
    """
    if now_ms is None:
        now_ms = int(time.time() * 1000)
    start = start_time_ms if start_time_ms is not None else now_ms
    end = end_time_ms if end_time_ms is not None else start + 30 * 24 * 60 * 60 * 1000
    if end <= start:
        raise PublishError("测试结束时间必须晚于开始时间。")
    if end - start > 90 * 24 * 60 * 60 * 1000:
        raise PublishError("测试周期不能超过 90 天。")
    return start, end


def collect_groups(data: dict[str, Any]) -> list[tuple[str, str]]:
    seen: set[str] = set()
    groups: list[tuple[str, str]] = []
    for obj in iter_dicts(data):
        group_id = obj.get("groupId")
        if not isinstance(group_id, (str, int)):
            continue
        group_id = str(group_id)
        if not group_id or group_id in seen:
            continue
        seen.add(group_id)
        name = (
            obj.get("groupName")
            or obj.get("name")
            or obj.get("testGroupName")
            or ""
        )
        groups.append((group_id, str(name)))
    return groups


def collect_test_versions(data: dict[str, Any]) -> list[tuple[str, str]]:
    """Collect HarmonyOS test version IDs from publish version-list output.

    AGC has changed the exact response nesting over time, so this deliberately
    accepts several marker fields while requiring a versionId and a positive
    indication that the entry is a test/HarmonyOS-test version.
    """
    seen: set[str] = set()
    versions: list[tuple[str, str]] = []
    for obj in iter_dicts(data):
        version_id = obj.get("versionId")
        if not isinstance(version_id, (str, int)) or not str(version_id):
            continue

        release_type = obj.get("releaseType")
        test_type = obj.get("testType")
        labels = " ".join(
            str(obj.get(key, ""))
            for key in (
                "versionType",
                "versionTypeName",
                "releaseTypeName",
                "testTypeName",
                "typeName",
            )
        ).lower()
        is_test = (
            str(release_type) == "6"
            or str(test_type) in {"3", "4"}
            or "test" in labels
            or "测试" in labels
        )
        if not is_test:
            continue

        version_id = str(version_id)
        if version_id in seen:
            continue
        seen.add(version_id)
        state = next(
            (
                str(obj[key])
                for key in ("state", "status", "versionState", "reviewState")
                if key in obj and obj[key] is not None
            ),
            "",
        )
        versions.append((version_id, state))
    return versions


def check_release_signing_config() -> None:
    try:
        text = BUILD_PROFILE.read_text(encoding="utf-8")
    except OSError as exc:
        raise PublishError(f"无法读取 {BUILD_PROFILE}: {exc}") from exc

    # Legacy/local mode: Hvigor signs from build-profile.json5. Passwords here
    # must already be the encrypted values produced by DevEco/Hvigor tooling.
    required = ("storeFile", "storePassword", "keyAlias", "keyPassword", "profile", "certpath")
    empty: list[str] = []
    for key in required:
        match = re.search(rf"\b{re.escape(key)}\s*:\s*(['\"])(.*?)\1", text, re.DOTALL)
        if not match or not match.group(2).strip():
            empty.append(key)
    if empty:
        raise PublishError(
            "release 签名配置还没填好："
            + ", ".join(empty)
            + "。本地 Hvigor 签名需要完整 build-profile；CI 建议使用 HARMONY_SIGN_*_FILE "
            "环境变量走 unsigned APP + hap-sign-tool 直接签名。"
        )


DIRECT_SIGN_ENV = {
    "p12": "HARMONY_SIGN_P12_FILE",
    "cer": "HARMONY_SIGN_CER_FILE",
    "profile": "HARMONY_SIGN_PROFILE_FILE",
    "store_password": "HARMONY_SIGN_STORE_PASSWORD",
    "key_alias": "HARMONY_SIGN_KEY_ALIAS",
    "key_password": "HARMONY_SIGN_KEY_PASSWORD",
}


def resolve_direct_signing() -> dict[str, str] | None:
    values = {key: os.environ.get(env_name, "").strip() for key, env_name in DIRECT_SIGN_ENV.items()}
    if not any(values.values()):
        return None

    missing = [DIRECT_SIGN_ENV[key] for key, value in values.items() if not value]
    if missing:
        raise PublishError("CI 直接签名参数不完整：" + ", ".join(missing))

    for key in ("p12", "cer", "profile"):
        path = Path(values[key]).expanduser().resolve()
        if not path.is_file():
            raise PublishError(f"CI 直接签名文件不存在：{DIRECT_SIGN_ENV[key]}={path}")
        values[key] = str(path)
    return values


def disable_hvigor_signing_text(text: str) -> str:
    """Detach the product from signingConfigs so Hvigor emits an unsigned APP."""
    pattern = r"(\bsigningConfig\s*:\s*)(['\"])(.*?)\2"

    def replacement(match: re.Match[str]) -> str:
        quote = match.group(2)
        return f"{match.group(1)}{quote}{quote}"

    updated, count = re.subn(pattern, replacement, text, count=1)
    if count != 1:
        raise PublishError("无法在 build-profile.json5 中定位 products[].signingConfig。")
    return updated


def resolve_hvigorw() -> str:
    hvigorw = shutil.which("hvigorw")
    if hvigorw:
        return hvigorw
    raise PublishError(
        "找不到 HarmonyOS CLI 提供的 hvigorw。"
        "先运行 tools/setup_harmony_cli.sh 并把 $HARMONY_CLI_HOME/bin 加入 PATH。"
    )


def resolve_hap_sign_tool() -> Path:
    candidates: list[Path] = []
    sdk_home = os.environ.get("DEVECO_SDK_HOME")
    if sdk_home:
        candidates.append(
            Path(sdk_home).expanduser()
            / "default"
            / "openharmony"
            / "toolchains"
            / "lib"
            / "hap-sign-tool.jar"
        )
    cli_home = os.environ.get("HARMONY_CLI_HOME")
    if cli_home:
        candidates.append(
            Path(cli_home).expanduser()
            / "sdk"
            / "default"
            / "openharmony"
            / "toolchains"
            / "lib"
            / "hap-sign-tool.jar"
        )
    for path in candidates:
        if path.is_file():
            return path.resolve()
    raise PublishError("找不到 SDK 自带的 hap-sign-tool.jar，无法对 unsigned APP 直接签名。")


def run_sensitive(
    cmd: Sequence[str],
    *,
    sensitive_values: Sequence[str],
    cwd: Path = ROOT,
) -> None:
    secrets = {value for value in sensitive_values if value}
    masked = ["***" if str(part) in secrets else shlex.quote(str(part)) for part in cmd]
    eprint("+", " ".join(masked))
    proc = subprocess.run(
        [str(part) for part in cmd],
        cwd=cwd,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise PublishError(f"签名命令失败（exit={proc.returncode}）：{cmd[0]}")


def find_latest_app() -> Path:
    candidates = [
        path
        for path in (HARMONY_DIR / "build").rglob("*.app")
        if path.is_file()
    ]
    if not candidates:
        raise PublishError("assembleApp 成功后没有找到 .app 产物。")
    return max(candidates, key=lambda p: p.stat().st_mtime)


def verify_release_app(app_path: Path) -> None:
    """Reject a package when archive metadata positively identifies a debug build.

    Huawei's APP container layout has changed across toolchain generations, so lack
    of readable metadata is a warning rather than a false hard failure.
    """
    if app_path.suffix.lower() != ".app":
        raise PublishError(f"AGC 测试分发必须上传 .app，不是：{app_path.name}")
    try:
        with zipfile.ZipFile(app_path) as outer:
            nested_haps = [name for name in outer.namelist() if name.lower().endswith(".hap")]
            observed_build_modes: set[str] = set()
            observed_debug: set[bool] = set()

            def inspect_json(raw: bytes) -> None:
                try:
                    obj = json.loads(raw.decode("utf-8"))
                except (UnicodeDecodeError, json.JSONDecodeError):
                    return
                for item in iter_dicts(obj):
                    mode = item.get("buildMode")
                    if isinstance(mode, str):
                        observed_build_modes.add(mode.lower())
                    debug = item.get("debug")
                    if isinstance(debug, bool):
                        observed_debug.add(debug)

            for name in outer.namelist():
                if name.endswith("pack.info") or name.endswith("module.json"):
                    inspect_json(outer.read(name))

            for hap_name in nested_haps:
                try:
                    from io import BytesIO

                    with zipfile.ZipFile(BytesIO(outer.read(hap_name))) as hap:
                        for name in hap.namelist():
                            if name.endswith("module.json") or name.endswith("pack.info"):
                                inspect_json(hap.read(name))
                except (zipfile.BadZipFile, KeyError):
                    continue

            if "debug" in observed_build_modes or True in observed_debug:
                raise PublishError("检测到 APP 内含 debug 构建信息，拒绝上传测试分发。")
            if observed_build_modes and "release" not in observed_build_modes:
                raise PublishError(
                    f"APP 构建模式不是 release：{sorted(observed_build_modes)}"
                )
            if not observed_build_modes and not observed_debug:
                eprint("警告：未从 APP 内解析到 buildMode/debug 元数据；继续依赖 release 构建命令与签名门禁。")
    except zipfile.BadZipFile:
        eprint("警告：当前工具链的 .app 不是标准 ZIP 容器，跳过内部元数据检查。")


def sign_release_app(unsigned_app: Path, signing: dict[str, str]) -> Path:
    java = shutil.which("java")
    if not java:
        raise PublishError("找不到 java，无法运行 hap-sign-tool.jar。")
    sign_tool = resolve_hap_sign_tool()

    base_name = unsigned_app.stem
    if base_name.endswith("-unsigned"):
        base_name = base_name[: -len("-unsigned")]
    signed_app = unsigned_app.with_name(base_name + "-ci-signed.app")

    run_sensitive(
        [
            java,
            "-jar",
            str(sign_tool),
            "sign-app",
            "-keyAlias",
            signing["key_alias"],
            "-signAlg",
            "SHA256withECDSA",
            "-mode",
            "localSign",
            "-appCertFile",
            signing["cer"],
            "-profileFile",
            signing["profile"],
            "-inFile",
            str(unsigned_app),
            "-keystoreFile",
            signing["p12"],
            "-outFile",
            str(signed_app),
            "-keyPwd",
            signing["key_password"],
            "-keystorePwd",
            signing["store_password"],
        ],
        sensitive_values=(signing["key_password"], signing["store_password"]),
        cwd=HARMONY_DIR,
    )
    if not signed_app.is_file():
        raise PublishError("hap-sign-tool 返回成功，但没有生成 signed .app。")

    with tempfile.TemporaryDirectory(prefix="sujian-harmony-verify-") as temp_dir:
        temp = Path(temp_dir)
        run(
            [
                java,
                "-jar",
                str(sign_tool),
                "verify-app",
                "-inFile",
                str(signed_app),
                "-outCertChain",
                str(temp / "cert-chain.cer"),
                "-outProfile",
                str(temp / "profile.p7b"),
            ],
            cwd=HARMONY_DIR,
        )
    eprint(f"APP 直接签名并验签成功：{signed_app.name}")
    return signed_app


def build_release_app(skip_rust: bool) -> Path:
    direct_signing = resolve_direct_signing()
    original_profile: str | None = None

    if direct_signing is None:
        check_release_signing_config()
    else:
        original_profile = BUILD_PROFILE.read_text(encoding="utf-8")
        BUILD_PROFILE.write_text(
            disable_hvigor_signing_text(original_profile),
            encoding="utf-8",
        )
        eprint("CI 直接签名模式：Hvigor 仅构建 unsigned APP，签名交给 hap-sign-tool.jar。")

    try:
        if not skip_rust:
            run([str(ROOT / "tools" / "build_harmony.sh")])
        hvigorw = resolve_hvigorw()
        run(
            [
                hvigorw,
                "--mode",
                "project",
                "-p",
                "product=default",
                "-p",
                "buildMode=release",
                "assembleApp",
                "--no-daemon",
            ],
            cwd=HARMONY_DIR,
        )
        app_path = find_latest_app()
    finally:
        if original_profile is not None:
            BUILD_PROFILE.write_text(original_profile, encoding="utf-8")

    if direct_signing is not None:
        app_path = sign_release_app(app_path, direct_signing)
    verify_release_app(app_path)
    return app_path


def resolve_cli() -> list[str]:
    explicit = os.environ.get("CONNECT_API_CLI")
    if explicit:
        return shlex.split(explicit)

    explicit_js = os.environ.get("CONNECT_API_CLI_JS")
    if explicit_js:
        path = Path(explicit_js).expanduser()
        if not path.is_file():
            raise PublishError(f"CONNECT_API_CLI_JS 不存在：{path}")
        return ["node", str(path)]

    embedded_candidates = [
        ROOT / ".opencode" / "skills" / "hmos-connect-api-cli-skill" / "scripts" / "connect-api-cli.js",
        ROOT / ".opencode" / "skills" / "harmonyos" / "launch-and-distribute"
        / "hmos-connect-api-cli-skill" / "scripts" / "connect-api-cli.js",
    ]
    for path in embedded_candidates:
        if path.is_file():
            return ["node", str(path)]

    installed = shutil.which("connect-api-cli")
    if installed:
        return [installed]

    npx = shutil.which("npx")
    if npx:
        version = os.environ.get(
            "CONNECT_API_CLI_NPM_VERSION", DEFAULT_CONNECT_API_CLI_NPM_VERSION
        )
        return [npx, "--yes", f"connect-api-cli@{version}"]

    raise PublishError(
        "找不到 connect-api-cli。安装 Node.js/npm，或设置 CONNECT_API_CLI / "
        "CONNECT_API_CLI_JS。"
    )


class AgcCli:
    def __init__(self, command: Sequence[str]) -> None:
        self.command = list(command)

    def raw(self, *args: str, json_result: bool = True) -> dict[str, Any] | str:
        eprint("+", " ".join(shlex.quote(part) for part in [*self.command, *args]))
        proc = subprocess.run(
            [*self.command, *args],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        _emit_safe_stderr(proc.stderr)
        stdout = proc.stdout or ""
        if proc.returncode != 0:
            # connect-api-cli >=1.1.4 exits non-zero for business failures, but
            # still writes the response JSON to stdout. Surface ret.msg/rtnDesc
            # when available instead of collapsing everything to "node failed".
            if stdout.strip():
                try:
                    data = parse_last_json(stdout)
                    validate_business_result(data, " ".join(args[:2]))
                except PublishError as exc:
                    raise exc
            raise PublishError(
                f"命令失败（exit={proc.returncode}）：{' '.join(args[:2])}"
            )
        if not json_result:
            return stdout
        data = parse_last_json(stdout)
        validate_business_result(data, " ".join(args[:2]))
        return data

    def try_raw(self, *args: str) -> tuple[bool, dict[str, Any] | None]:
        """Run an AGC command whose failure is an expected state probe.

        Used by old-version cleanup where "not reviewing", "not running", or
        "not deletable" are normal outcomes. Expected failures are not retried.
        """
        proc = subprocess.run(
            [*self.command, *args],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        if proc.returncode != 0:
            return False, None
        try:
            data = parse_last_json(proc.stdout or "")
            validate_business_result(data, " ".join(args[:2]))
        except PublishError:
            return False, None
        return True, data

    def verify(self) -> None:
        proc = run([*self.command, "--version"], capture=True)
        version_output = (proc.stdout or proc.stderr or "").strip()
        if version_output:
            eprint(f"connect-api-cli: {version_output.splitlines()[-1]}")

        # Do this capability gate before the first AGC write. The npm package and
        # the DevEco embedded skill are released independently; an older npm build
        # may exist but not yet contain all Testing API commands.
        probe = subprocess.run(
            [*self.command, "test", "--help"],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        test_help = (probe.stdout or "") + "\n" + (probe.stderr or "")
        required_commands = (
            "version-create",
            "pkg-add",
            "version-update",
            "version-submit",
            "version-stop",
            "version-delete",
            "group-list",
        )
        missing = [name for name in required_commands if name not in test_help]

        publish_probe = subprocess.run(
            [*self.command, "publish", "--help"],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        publish_help = (publish_probe.stdout or "") + "\n" + (publish_probe.stderr or "")
        required_publish_commands = ("app-id", "version-list", "cancel-review")
        missing_publish = [
            name for name in required_publish_commands if name not in publish_help
        ]
        if (
            probe.returncode != 0
            or publish_probe.returncode != 0
            or missing
            or missing_publish
        ):
            details = [*missing, *missing_publish]
            raise PublishError(
                "当前 connect-api-cli 不包含完整测试发布/替换命令"
                + (f"（缺少：{', '.join(details)}）" if details else "")
                + "。优先安装官方 hmos-connect-api-cli-skill，或设置 "
                "CONNECT_API_CLI_JS 指向该 skill 的 scripts/connect-api-cli.js。"
            )

        # auth status may be text rather than JSON. A missing/invalid credential must
        # fail here before any remote write.
        proc = run([*self.command, "auth", "status"], capture=True)
        _emit_safe_stderr(proc.stderr)
        combined = ((proc.stdout or "") + "\n" + (proc.stderr or "")).lower()
        missing_markers = (
            "missing agc credentials",
            "not logged in",
            "auth status unavailable",
        )
        if any(marker in combined for marker in missing_markers):
            raise PublishError(
                "AGC 凭据不可用。把凭据写到仓库根目录 .env（已被 Git 忽略），"
                "或导出对应环境变量。"
            )

    def resolve_app_id(self, package_name: str, explicit: str | None) -> str:
        if explicit:
            return explicit
        data = self.raw("publish", "app-id", "-p", package_name)
        assert isinstance(data, dict)
        app_id = first_value(data, ("appId", "value"))
        if not app_id:
            raise PublishError(f"AGC 中找不到包名 {package_name} 对应的 appId。")
        return app_id

    def resolve_group_id(
        self,
        app_id: str,
        explicit_id: str | None,
        group_name: str | None,
    ) -> str:
        if explicit_id:
            return explicit_id

        data = self.raw("test", "group-list", "-a", app_id, "--page-size", "100")
        assert isinstance(data, dict)
        groups = collect_groups(data)
        if group_name:
            matches = [item for item in groups if item[1] == group_name]
            if len(matches) == 1:
                return matches[0][0]
            if not matches:
                raise PublishError(f"没有找到测试群组：{group_name}")
            raise PublishError(f"测试群组名称重复：{group_name}，请直接指定 groupId。")
        if len(groups) == 1:
            return groups[0][0]
        if not groups:
            raise PublishError(
                "AGC 里没有可用测试群组。先创建并添加测试成员，再设置 "
                "AGC_TEST_GROUP_ID。"
            )
        summary = ", ".join(
            f"{name or '(未命名)'}={group_id}" for group_id, name in groups[:10]
        )
        raise PublishError(
            "发现多个测试群组，不能替你猜。设置 AGC_TEST_GROUP_ID 或 "
            f"AGC_TEST_GROUP_NAME。当前群组：{summary}"
        )


def cleanup_old_test_versions(
    cli: AgcCli,
    *,
    app_id: str,
    package_name: str,
) -> list[str]:
    """Best-effort retire old HarmonyOS test versions before publishing a new one.

    For each test version we first try to cancel review, then stop an effective
    test, and finally delete it. AGC legitimately rejects operations that do not
    match the current state, so those probe failures are ignored. A version that
    cannot be changed at all is retained as history and the new publish attempt
    continues; AGC itself remains the final authority on whether a new version is
    allowed.
    """
    data = cli.raw(
        "publish",
        "version-list",
        "-a",
        app_id,
        "-p",
        package_name,
    )
    assert isinstance(data, dict)
    versions = collect_test_versions(data)
    if not versions:
        eprint("AGC 没有旧的 HarmonyOS 测试版本需要处理。")
        return []

    removed: list[str] = []
    for version_id, state in versions:
        label = f"{version_id}（state={state}）" if state else version_id
        eprint(f"处理旧测试版本：{label}")

        cancelled, _ = cli.try_raw(
            "publish",
            "cancel-review",
            "-a",
            app_id,
            "-v",
            version_id,
        )
        if cancelled:
            eprint(f"  已撤销审核：{version_id}")

        stopped, _ = cli.try_raw(
            "test",
            "version-stop",
            "-a",
            app_id,
            "-v",
            version_id,
        )
        if stopped:
            eprint(f"  已停止测试：{version_id}")

        deleted, _ = cli.try_raw(
            "test",
            "version-delete",
            "-a",
            app_id,
            "-v",
            version_id,
        )
        if deleted:
            removed.append(version_id)
            eprint(f"  已删除旧测试版本：{version_id}")
        elif cancelled or stopped:
            eprint(f"  旧版本已退出活动状态，但 AGC 当前不允许删除：{version_id}")
        else:
            eprint(f"  旧版本当前不可撤销/停止/删除，作为历史记录保留：{version_id}")

    return removed


def wait_for_package_compile(
    cli: "AgcCli",
    *,
    app_id: str,
    pkg_id: str,
    attempts: int = 30,
    delay_seconds: float = 10.0,
) -> None:
    """Wait until AGC finishes parsing the uploaded package."""
    last_status = ""
    for attempt in range(attempts):
        data = cli.raw(
            "publish",
            "compile-status",
            "-a",
            app_id,
            "--pkg-ids",
            pkg_id,
        )
        assert isinstance(data, dict)
        status = first_value(data, ("successStatus",))
        if status == "0":
            eprint(f"软件包解析完成：{pkg_id}")
            return
        last_status = status or "unknown"
        eprint(f"软件包仍在解析：successStatus={last_status}")
        if attempt < attempts - 1:
            time.sleep(delay_seconds)
    raise PublishError(
        f"等待软件包解析超时：pkgId={pkg_id}，successStatus={last_status}"
    )


def build_version_update_body(
    *,
    version_id: str,
    pkg_id: str | None,
    test_desc: str,
    test_type: int,
    group_id: str | None,
    start_time_ms: int | None,
    end_time_ms: int | None,
    notify: bool,
) -> dict[str, Any]:
    body: dict[str, Any] = {"versionId": version_id}
    if pkg_id:
        body["pkgId"] = pkg_id
    open_test: dict[str, Any] = {"testDesc": test_desc}
    if start_time_ms is not None:
        open_test["startTime"] = start_time_ms
    if end_time_ms is not None:
        open_test["endTime"] = end_time_ms

    if test_type == 3:
        if not group_id:
            raise PublishError("邀请测试必须绑定测试群组。")
        open_test["testTaskInfo"] = {
            "groupInfos": [{"groupId": group_id}],
            "needShareLink": 0,
            "displayArea": "1",
            "needNotify": 1 if notify else 0,
        }

    body["openTestInfo"] = open_test
    return body


def publish(args: argparse.Namespace) -> None:
    cli = AgcCli(resolve_cli())
    cli.verify()

    app_id = cli.resolve_app_id(args.package_name, args.app_id)
    eprint(f"AGC appId: {app_id}")

    app_path = Path(args.app).expanduser().resolve() if args.app else build_release_app(args.skip_rust)
    if not app_path.is_file():
        raise PublishError(f"APP 不存在：{app_path}")
    verify_release_app(app_path)
    eprint(f"APP: {app_path}")

    group_id: str | None = None
    if args.test_type == 3:
        group_id = cli.resolve_group_id(app_id, args.group_id, args.group_name)
        eprint(f"测试群组 ID: {group_id}")

    if not args.keep_old_versions:
        cleanup_old_test_versions(
            cli,
            app_id=app_id,
            package_name=args.package_name,
        )

    created = cli.raw(
        "test",
        "version-create",
        "-a",
        app_id,
        "--test-type",
        str(args.test_type),
        "--test-desc",
        args.test_desc,
        "-r",
        "6",
    )
    assert isinstance(created, dict)
    version_id = first_value(created, ("versionId",))
    if not version_id:
        raise PublishError("测试版本已创建，但响应里没有 versionId；停止继续写入。")
    eprint(f"测试版本 ID: {version_id}")

    uploaded = cli.raw(
        "upload",
        "file",
        "-a",
        app_id,
        "-f",
        str(app_path),
        "-r",
        "6",
    )
    assert isinstance(uploaded, dict)
    object_id = first_value(uploaded, ("objectId",))
    file_name = first_value(uploaded, ("fileName",)) or app_path.name
    if not object_id:
        raise PublishError("APP 上传响应里没有 objectId；停止继续写入。")

    package = cli.raw(
        "test",
        "pkg-add",
        "-a",
        app_id,
        "--file-name",
        file_name,
        "--object-id",
        object_id,
        "--distribute-mode",
        str(args.distribute_mode),
    )
    assert isinstance(package, dict)
    pkg_id = (
        first_value(package, ("pkgId", "packageId", "packageID", "pkgID"))
        or first_list_value(package, ("pkgVersion",))
    )
    if not pkg_id:
        # The current Testing API may acknowledge pkg-add with only ret.code=0.
        # Query the just-created version to recover the attached package ID.
        for attempt in range(3):
            versions = cli.raw(
                "publish",
                "version-list",
                "-a",
                app_id,
                "-p",
                args.package_name,
            )
            assert isinstance(versions, dict)
            pkg_id = find_version_package_id(versions, version_id)
            if pkg_id:
                break
            if attempt < 2:
                time.sleep(2)

    if not pkg_id:
        raise PublishError(
            "添加测试软件包成功，但响应中没有 pkgId/packageId/pkgVersion，"
            "且 version-list 也无法反查；不能提交一个没有绑定软件包的测试版本。"
        )

    eprint(f"测试软件包 ID: {pkg_id}")
    wait_for_package_compile(cli, app_id=app_id, pkg_id=pkg_id)

    start_time_ms, end_time_ms = normalize_test_window(
        args.start_time_ms,
        args.end_time_ms,
    )
    eprint(f"测试时间窗：{start_time_ms} -> {end_time_ms}")

    body = build_version_update_body(
        version_id=version_id,
        pkg_id=pkg_id,
        test_desc=args.test_desc,
        test_type=args.test_type,
        group_id=group_id,
        start_time_ms=start_time_ms,
        end_time_ms=end_time_ms,
        notify=args.notify,
    )
    with tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        suffix=".json",
        prefix="sujian-agc-test-",
        delete=False,
    ) as handle:
        json.dump(body, handle, ensure_ascii=False, separators=(",", ":"))
        body_path = Path(handle.name)
    try:
        cli.raw("test", "version-update", "-a", app_id, "--body", str(body_path))
    finally:
        body_path.unlink(missing_ok=True)

    if args.no_submit:
        print(
            json.dumps(
                {
                    "appId": app_id,
                    "versionId": version_id,
                    "pkgId": pkg_id,
                    "submitted": False,
                },
                ensure_ascii=False,
            )
        )
        return

    cli.raw("test", "version-submit", "-a", app_id, "-v", version_id)
    print(
        json.dumps(
            {
                "appId": app_id,
                "versionId": version_id,
                "pkgId": pkg_id,
                "submitted": True,
            },
            ensure_ascii=False,
        )
    )


def default_description() -> str:
    sha = "local"
    try:
        proc = subprocess.run(
            ["git", "rev-parse", "--short=8", "HEAD"],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if proc.returncode == 0 and proc.stdout.strip():
            sha = proc.stdout.strip()
    except OSError:
        pass
    return f"素笺自动测试版 {sha}"[:50]


def env_int(name: str) -> int | None:
    value = os.environ.get(name)
    if value is None or not value.strip():
        return None
    try:
        return int(value)
    except ValueError as exc:
        raise PublishError(f"{name} 必须是整数毫秒时间戳。") from exc


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="在 Linux 上构建并发布素笺 HarmonyOS AGC 测试版。"
    )
    parser.add_argument(
        "--app",
        help="跳过 assembleApp，直接上传已有 release 签名 .app。",
    )
    parser.add_argument(
        "--app-id",
        default=os.environ.get("AGC_APP_ID"),
        help="AGC App ID；默认按包名自动查询。",
    )
    parser.add_argument(
        "--package-name",
        default=os.environ.get("AGC_PACKAGE_NAME", DEFAULT_PACKAGE_NAME),
    )
    parser.add_argument(
        "--test-type",
        type=int,
        choices=(3, 4),
        default=int(os.environ.get("AGC_TEST_TYPE", "3")),
        help="3=邀请测试（默认），4=公开测试。",
    )
    parser.add_argument(
        "--test-desc",
        default=os.environ.get("AGC_TEST_DESC") or default_description(),
        help="测试说明，华为限制最多 50 个字符。",
    )
    parser.add_argument(
        "--group-id",
        default=os.environ.get("AGC_TEST_GROUP_ID"),
        help="邀请测试群组 ID。未指定时尝试唯一群组。",
    )
    parser.add_argument(
        "--group-name",
        default=os.environ.get("AGC_TEST_GROUP_NAME"),
        help="按群组名精确匹配；有多个群组时推荐设置。",
    )
    parser.add_argument(
        "--distribute-mode",
        type=int,
        choices=(1, 2),
        default=int(os.environ.get("AGC_DISTRIBUTE_MODE", "1")),
        help="1=测试专区（默认），2=AppGallery。",
    )
    parser.add_argument(
        "--start-time-ms",
        type=int,
        default=env_int("AGC_TEST_START_TIME_MS"),
        help="可选测试开始时间，Unix 毫秒。",
    )
    parser.add_argument(
        "--end-time-ms",
        type=int,
        default=env_int("AGC_TEST_END_TIME_MS"),
        help="可选测试结束时间，Unix 毫秒。",
    )
    parser.add_argument(
        "--notify",
        action="store_true",
        default=os.environ.get("AGC_TEST_NOTIFY") == "1",
        help="提交的测试信息中请求通知测试成员。",
    )
    parser.add_argument(
        "--skip-rust",
        action="store_true",
        help="不重编 Rust FFI；仅在 prebuilt 已确认最新时使用。",
    )
    parser.add_argument(
        "--keep-old-versions",
        action="store_true",
        default=os.environ.get("AGC_KEEP_OLD_TEST_VERSIONS") == "1",
        help="不自动撤销/停止/删除旧 HarmonyOS 测试版本。",
    )
    parser.add_argument(
        "--no-submit",
        action="store_true",
        help="完成创建/上传/绑定，但不提交审核。",
    )
    args = parser.parse_args(argv)
    if len(args.test_desc) > 50:
        parser.error("--test-desc 不能超过 50 个字符。")
    if args.start_time_ms is not None and args.end_time_ms is not None:
        if args.end_time_ms <= args.start_time_ms:
            parser.error("--end-time-ms 必须晚于 --start-time-ms。")
    return args


def main(argv: Sequence[str] | None = None) -> int:
    try:
        args = parse_args(argv if argv is not None else sys.argv[1:])
        publish(args)
        return 0
    except PublishError as exc:
        eprint(f"错误：{exc}")
        return 1
    except KeyboardInterrupt:
        eprint("已取消。")
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
