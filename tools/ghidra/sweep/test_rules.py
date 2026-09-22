#!/usr/bin/env python3
"""Checks for the name rules. Uses synthetic records only."""

import tempfile
import unittest
from pathlib import Path

from sweep_lib import (
    LABEL_INFERRED,
    LABEL_OBSERVED,
    LABEL_UNKNOWN,
    MAX_NAMING_STRING_FUNCS,
    RULE_HELPER,
    RULE_NETWORK,
    RULE_STRING,
    RULE_VTABLE,
    RULE_WRAPPER,
    TypeCatalog,
    apply_rules,
    iter_records,
    write_tsv,
)
import gzip
import json


ZERO = "0" * 32
DEATH = "AB" * 16


def catalog():
    return TypeCatalog.from_pairs([ZERO, DEATH], {DEATH: "PlayerComponentClientFacet_OnPlayerDeath"})


def rec(**kwargs):
    base = {
        "index": 0,
        "entry": "0x140000100",
        "name": "FUN_140000100",
        "namespace": "Global",
        "size": 16,
        "is_thunk": False,
        "thunk_target": "",
        "thunk_target_name": "",
        "is_external": False,
        "callers": [],
        "callees": [],
        "strings": [],
        "rtti": [],
        "vtables": [],
        "has_c": True,
        "error": "",
    }
    base.update(kwargs)
    return base


class RuleTests(unittest.TestCase):
    def test_log_string_names_the_function(self):
        rows = apply_rules([
            rec(strings=["PlayerTradeComponent - CanTakeItems"]),
        ], catalog())
        self.assertEqual(rows[0]["suggested_name"], "PlayerTradeComponent::CanTakeItems")
        self.assertEqual(rows[0]["label"], LABEL_OBSERVED)
        self.assertEqual(rows[0]["rule"], RULE_STRING)

    def test_container_string_does_not_name_the_function(self):
        rows = apply_rules([rec(strings=["AZStd::allocator"])], catalog())
        self.assertEqual(rows[0]["label"], LABEL_UNKNOWN)

    def test_format_string_does_not_name_the_function(self):
        rows = apply_rules([rec(strings=["%s::%s::Setter"])], catalog())
        self.assertEqual(rows[0]["label"], LABEL_UNKNOWN)

    def test_common_string_does_not_name_the_function(self):
        shared = "Javelin::Shared::Check"
        records = [
            rec(entry=hex(0x140000100 + step), name="FUN_" + hex(0x140000100 + step)[2:],
                strings=[shared])
            for step in range(MAX_NAMING_STRING_FUNCS + 1)
        ]
        rows = apply_rules(records, catalog())
        self.assertTrue(all(row["rule"] != RULE_STRING for row in rows))

    def test_rare_string_still_names_the_function(self):
        rows = apply_rules([rec(strings=["Javelin::DoorComponent::Open"])], catalog())
        self.assertEqual(rows[0]["suggested_name"], "Javelin::DoorComponent::Open")
        self.assertEqual(rows[0]["rule"], RULE_STRING)

    def test_type_name_template_names_the_class(self):
        rows = apply_rules([
            rec(strings=[
                "const char *__cdecl MB::GetTypeName<class Javelin::CameraLockTargetComponent>(void)"
            ]),
        ], catalog())
        self.assertEqual(
            rows[0]["suggested_name"], "Javelin::CameraLockTargetComponent::GetTypeName"
        )
        self.assertEqual(rows[0]["rule"], RULE_STRING)

    def test_null_uuid_is_not_a_network_type(self):
        rows = apply_rules([rec(strings=[ZERO])], catalog())
        self.assertEqual(rows[0]["label"], LABEL_UNKNOWN)
        self.assertIsNone(catalog().slot_for_uuid(ZERO))

    def test_vtable_slot_uses_rtti(self):
        rows = apply_rules([
            rec(vtables=[{
                "slot": "0x140010028",
                "vtable": "0x140010000",
                "symbol": ".?AVTradeComponent@@6B@",
                "namespace": "Global",
                "slot_index": 5,
            }]),
        ], catalog())
        self.assertEqual(rows[0]["suggested_name"], "TradeComponent::vfunc_5")
        self.assertEqual(rows[0]["label"], LABEL_OBSERVED)
        self.assertEqual(rows[0]["rule"], RULE_VTABLE)

    def test_unmarshal_is_a_reader(self):
        from sweep_lib import role_of
        self.assertEqual(role_of("FooUnmarshal"), "reader")
        self.assertEqual(role_of("FooMarshal"), "writer")

    def test_short_facet_joins_qualified_catalog_name(self):
        qualified = TypeCatalog.from_pairs(
            [ZERO, DEATH],
            {DEATH: "Javelin::ClientMessages::PlayerComponentClientFacet_OnPlayerDeath"},
        )
        rows = apply_rules([
            rec(strings=["PlayerComponentClientFacet_OnPlayerDeath"]),
        ], qualified)
        self.assertEqual(
            rows[0]["suggested_name"],
            "Javelin::ClientMessages::PlayerComponentClientFacet_OnPlayerDeath",
        )
        self.assertIn("index 1", rows[0]["evidence"])

    def test_network_type_from_facet_string(self):
        rows = apply_rules([
            rec(strings=["PlayerComponentClientFacet_OnPlayerDeath"]),
        ], catalog())
        self.assertEqual(rows[0]["suggested_name"], "PlayerComponentClientFacet_OnPlayerDeath")
        self.assertEqual(rows[0]["label"], LABEL_OBSERVED)
        self.assertEqual(rows[0]["rule"], RULE_NETWORK)
        self.assertIn("index 1", rows[0]["evidence"])
        self.assertIn("role handler", rows[0]["evidence"])

    def test_thunk_uses_callee_name(self):
        rows = apply_rules([
            rec(entry="0x140000200", name="NamedTarget", namespace="Global"),
            rec(
                entry="0x140000100",
                name="thunk_FUN_140000100",
                is_thunk=True,
                thunk_target="0x140000200",
                thunk_target_name="NamedTarget",
            ),
        ], catalog())
        thunk = rows[1]
        self.assertEqual(thunk["suggested_name"], "NamedTarget_thunk")
        self.assertEqual(thunk["label"], LABEL_INFERRED)
        self.assertEqual(thunk["rule"], RULE_WRAPPER)

    def test_one_call_wrapper(self):
        rows = apply_rules([
            rec(entry="0x140000200", name="NamedTarget"),
            rec(entry="0x140000100", callees=["0x140000200"]),
        ], catalog())
        self.assertEqual(rows[1]["suggested_name"], "NamedTarget_wrapper")
        self.assertEqual(rows[1]["rule"], RULE_WRAPPER)

    def test_helper_propagation(self):
        rows = apply_rules([
            rec(entry="0x140000200", name="TradeComponent::CanTake", namespace="TradeComponent"),
            rec(entry="0x140000100", callers=["0x140000200"]),
        ], catalog())
        self.assertEqual(rows[1]["suggested_name"], "TradeComponent::helper_140000100")
        self.assertEqual(rows[1]["label"], LABEL_INFERRED)
        self.assertEqual(rows[1]["rule"], RULE_HELPER)

    def test_unknown_keeps_hint(self):
        rows = apply_rules([
            rec(strings=["just a log line"], callers=["0x140000999"]),
        ], catalog())
        self.assertEqual(rows[0]["label"], LABEL_UNKNOWN)
        self.assertIn("just a log line", rows[0]["evidence"])
        self.assertEqual(rows[0]["suggested_name"], "FUN_140000100")

    def test_gzip_reader_keeps_complete_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "batch-0.jsonl.gz"
            payload = {"entry": "0x1", "name": "FUN_1", "strings": ["ok"]}
            with gzip.open(path, "wt", encoding="utf-8") as handle:
                handle.write(json.dumps(payload) + "\n")
            rows = list(iter_records(path))
            self.assertEqual(rows[0]["entry"], "0x1")

    def test_tsv_round_trip(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "names.tsv"
            write_tsv(path, [{
                "entry": "0x1",
                "old_name": "FUN_1",
                "suggested_name": "A::b",
                "label": LABEL_OBSERVED,
                "rule": RULE_STRING,
                "evidence": "a\tb",
            }])
            text = path.read_text(encoding="utf-8")
            self.assertTrue(text.startswith("entry\t"))
            self.assertNotIn("a\tb", text.split("\n")[1])


if __name__ == "__main__":
    unittest.main()
