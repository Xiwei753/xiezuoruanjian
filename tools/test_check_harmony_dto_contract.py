#!/usr/bin/env python3

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("check_harmony_dto_contract.py")
SPEC = importlib.util.spec_from_file_location("check_harmony_dto_contract", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


RUST_DTO = """\
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSettingsDto {
    pub editor_font_size: f32,
    pub auto_save_enabled: bool,
    pub desktop_sidebar_width: f64,
    pub theme_mode: Option<String>,
    pub stats_device_id: Option<String>,
}
"""

ARKTS_OK = """\
export interface LocalSettings {
  editorFontSize: number
  autoSaveEnabled: boolean
  desktopSidebarWidth: number
  themeMode: string | null
  statsDeviceId: string | null
}
"""

DECODER_OK = """\
export function decodeLocalSettings(data: Object): DecodeResult<LocalSettings> {
  const editorFontSize: number | null = reqNum(data, 'editorFontSize')
  if (editorFontSize === null) { return err('editorFontSize missing') }
  const autoSaveEnabled: boolean | null = reqBool(data, 'autoSaveEnabled')
  if (autoSaveEnabled === null) { return err('autoSaveEnabled missing') }
  const desktopSidebarWidth: number | null = reqNum(data, 'desktopSidebarWidth')
  if (desktopSidebarWidth === null) { return err('desktopSidebarWidth missing') }
  return ok({
    editorFontSize: editorFontSize,
    autoSaveEnabled: autoSaveEnabled,
    desktopSidebarWidth: desktopSidebarWidth,
    themeMode: nullStr(data, 'themeMode'),
    statsDeviceId: nullStr(data, 'statsDeviceId')
  })
}
"""


class HarmonyDtoContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.original_interface_map = dict(MODULE.INTERFACE_MAP)
        self.original_decoder_map = dict(MODULE.DECODER_MAP)
        self.original_by_dto = dict(MODULE._INTERFACE_BY_DTO)

    def tearDown(self) -> None:
        MODULE.INTERFACE_MAP = self.original_interface_map
        MODULE.DECODER_MAP = self.original_decoder_map
        MODULE._INTERFACE_BY_DTO = self.original_by_dto

    def build(
        self,
        rust: str = RUST_DTO,
        arkts: str = ARKTS_OK,
        decoder: str = DECODER_OK,
        interface_key: str = "SettingsDtos.ets:LocalSettings",
        iface_name: str = "LocalSettings",
        dto_name: str = "LocalSettingsDto",
    ) -> list:
        MODULE.INTERFACE_MAP = {interface_key: dto_name}
        MODULE.DECODER_MAP = {"decodeLocalSettings": dto_name}
        MODULE._INTERFACE_BY_DTO = {dto_name: interface_key}
        dtos = MODULE.parse_rust_dtos_from_text(Path("sample.rs"), rust)
        parsed = MODULE.parse_arkts_interfaces_from_text(arkts)
        interfaces = {interface_key: parsed[iface_name]}
        decoders = MODULE.parse_decoders_from_text(decoder)
        return MODULE.check_sources(dtos, interfaces, decoders)

    def rules(self, findings: list) -> set[str]:
        return {finding.rule for finding in findings}

    # ------------------------------------------------------------------
    # 正测试
    # ------------------------------------------------------------------

    def test_aligned_dto_and_decoder_has_no_findings(self) -> None:
        self.assertEqual(self.build(), [])

    # ------------------------------------------------------------------
    # 规则 1：ArkTS interface <-> Core DTO 字段集合
    # ------------------------------------------------------------------

    def test_arkts_extra_field_is_reported(self) -> None:
        arkts = ARKTS_OK.replace(
            "  statsDeviceId: string | null\n}",
            "  statsDeviceId: string | null\n  legacySidebarWidth: number\n}",
        )
        findings = self.build(arkts=arkts)
        self.assertIn("dto-field-drift", self.rules(findings))
        self.assertTrue(any("legacySidebarWidth" in f.message for f in findings))

    def test_arkts_missing_field_is_reported(self) -> None:
        arkts = ARKTS_OK.replace("  desktopSidebarWidth: number\n", "")
        findings = self.build(arkts=arkts)
        self.assertIn("dto-field-drift", self.rules(findings))
        self.assertTrue(any("desktopSidebarWidth" in f.message for f in findings))

    # ------------------------------------------------------------------
    # 规则 2/3：decoder 键集合
    # ------------------------------------------------------------------

    def test_decoder_private_alias_is_reported(self) -> None:
        decoder = DECODER_OK.replace("reqNum(data, 'editorFontSize')", "reqNum(data, 'fontSize')")
        findings = self.build(decoder=decoder)
        self.assertIn("decoder-alias", self.rules(findings))
        self.assertTrue(any("'fontSize'" in f.message for f in findings))

    def test_decoder_dropped_field_is_reported(self) -> None:
        decoder = DECODER_OK.replace(
            "  const autoSaveEnabled: boolean | null = reqBool(data, 'autoSaveEnabled')\n"
            "  if (autoSaveEnabled === null) { return err('autoSaveEnabled missing') }\n",
            "",
        ).replace("    autoSaveEnabled: autoSaveEnabled,\n", "")
        findings = self.build(decoder=decoder)
        self.assertIn("decoder-missing-field", self.rules(findings))
        self.assertTrue(any("autoSaveEnabled" in f.message for f in findings))

    def test_decoder_missing_entirely_is_reported(self) -> None:
        findings = self.build(decoder="export function decodeSomethingElse(data: Object): DecodeResult<Object> {\n  return ok(data)\n}\n")
        self.assertIn("decoder-not-found", self.rules(findings))

    def test_unmapped_decoder_is_reported(self) -> None:
        decoder = DECODER_OK + "\nexport function decodeExtraThing(data: Object): DecodeResult<Object> {\n  return ok(data)\n}\n"
        findings = self.build(decoder=decoder)
        self.assertIn("unmapped-decoder", self.rules(findings))

    # ------------------------------------------------------------------
    # 规则 4：可空性
    # ------------------------------------------------------------------

    def test_nullable_field_must_use_null_helper(self) -> None:
        decoder = DECODER_OK.replace("nullStr(data, 'themeMode')", "optStr(data, 'themeMode')")
        findings = self.build(decoder=decoder)
        self.assertIn("decoder-nullability", self.rules(findings))
        self.assertTrue(any("themeMode" in f.message for f in findings))

    def test_required_field_must_use_req_helper(self) -> None:
        decoder = DECODER_OK.replace("reqNum(data, 'editorFontSize')", "optNum(data, 'editorFontSize')")
        findings = self.build(decoder=decoder)
        self.assertIn("decoder-nullability", self.rules(findings))

    def test_optional_field_must_use_opt_helper(self) -> None:
        arkts = ARKTS_OK.replace("  statsDeviceId: string | null\n", "  statsDeviceId?: string\n")
        decoder = DECODER_OK.replace("nullStr(data, 'statsDeviceId')", "reqStr(data, 'statsDeviceId')")
        findings = self.build(arkts=arkts, decoder=decoder)
        self.assertIn("decoder-nullability", self.rules(findings))

    # ------------------------------------------------------------------
    # 列表驱动键（decodeThemeColorScheme 的 string[] 循环）
    # ------------------------------------------------------------------

    def test_list_driven_keys_are_collected(self) -> None:
        decoder = """\
export function decodeThemeColorScheme(data: Object): DecodeResult<Object> {
  const required: string[] = [
    'primary', 'on_primary'
  ]
  const value: Object = {} as Object
  for (let i = 0; i < required.length; i++) {
    const key: string = required[i]
    const v: string | null = reqStr(data, key)
    if (v === null) { return err('missing') }
    (value as Record<string, string>)[key] = v
  }
  return ok(value)
}
"""
        decoders = MODULE.parse_decoders_from_text(decoder)
        self.assertEqual(decoders["decodeThemeColorScheme"].keys, {"primary": "req", "on_primary": "req"})

    # ------------------------------------------------------------------
    # 契约层 import：悬空类型/值
    # ------------------------------------------------------------------

    def test_export_parser_collects_declarations_and_reexports(self) -> None:
        text = """\
export interface A {
  x: number
}
export enum B { X = 'x' }
export function c(): void {}
export { D } from './d'
"""
        self.assertEqual(MODULE.parse_exported_names(text), {"A", "B", "c", "D"})

    def test_dangling_named_import_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dto_dir = root / MODULE.ARKTS_DTO_DIR
            dto_dir.mkdir(parents=True, exist_ok=True)
            (dto_dir / "ResultEnvelope.ets").write_text(
                "export interface ResultEnvelope<T> {\n  success: boolean\n}\n", encoding="utf-8"
            )
            (dto_dir.parent / "Consumer.ets").write_text(
                "import { ResultEnvelope, ChangedEntity } from './dto/ResultEnvelope'\n",
                encoding="utf-8",
            )
            findings = MODULE.check_contract_imports(root)
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].rule, "contract-import-missing")
        self.assertIn("ChangedEntity", findings[0].message)

    # ------------------------------------------------------------------
    # 集成：真实仓库当前是 GREEN
    # ------------------------------------------------------------------

    def test_repository_is_consistent(self) -> None:
        root = MODULE_PATH.parent.parent
        findings = MODULE.scan(root)
        self.assertEqual(
            findings,
            [],
            "Core DTO 与 Harmony DTO/decoder 漂移:\n"
            + "\n".join(f"{f.rule}: {f.where} — {f.message}" for f in findings),
        )


if __name__ == "__main__":
    unittest.main()
