#!/usr/bin/env python3
"""Write the sweep notes from batch files and names.tsv."""

import json
import sys
from collections import Counter, defaultdict
from datetime import datetime
from pathlib import Path

from sweep_lib import (
    LABEL_UNKNOWN,
    TypeCatalog,
    class_from_name,
    clean_namespace,
    iter_batch_paths,
    iter_records,
    observed_class,
    parse_rtti,
    read_tsv,
    role_of,
)

TOP_STRINGS = 2000
STRING_FUNCTION_LIMIT = 20
LARGEST_UNKNOWNS = 50
UNKNOWN_RANGE_BYTES = 0x400000


def load_json(path):
    path = Path(path)
    if not path.is_file():
        return {}
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def parse_time(text):
    if not text:
        return None
    try:
        return datetime.strptime(text, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError:
        return None


def format_duration(seconds):
    if seconds is None or seconds < 0:
        return "Unknown"
    hours = int(seconds // 3600)
    minutes = int((seconds % 3600) // 60)
    return "{} h {} min".format(hours, minutes)


def entry_int(entry):
    if not entry:
        return None
    if entry.startswith("0x"):
        try:
            return int(entry, 16)
        except ValueError:
            return None
    if ":" in entry:
        return None
    return None


def class_keys(record, suggested):
    keys = []
    namespace = clean_namespace(record.get("namespace") or "")
    if namespace:
        keys.append((namespace, "namespace"))
    for hit in record.get("vtables") or []:
        parsed = parse_rtti(hit.get("symbol") or "") or clean_namespace(hit.get("namespace") or "")
        if parsed:
            keys.append((parsed, "rtti"))
    if not keys:
        parsed = observed_class(record) or class_from_name(suggested)
        if parsed:
            source = "rtti" if observed_class(record) else "suggested name"
            keys.append((parsed, source))
    unique = []
    seen = set()
    for key in keys:
        if key[0] in seen:
            continue
        seen.add(key[0])
        unique.append(key)
    return unique


def write_index(path, facts):
    lines = ["# Sweep index", ""]
    lines.append("**Observed.** Program name: {}.".format(facts["program_name"]))
    lines.append("**Observed.** Image base: {}.".format(facts["image_base"]))
    lines.append("**Observed.** Function count in the program: {}.".format(facts["function_count"]))
    lines.append("**Observed.** Functions written to batch files: {}.".format(facts["written"]))
    lines.append("**Observed.** Functions with decompiled C text: {}.".format(facts["decompiled"]))
    lines.append("**Observed.** Functions with a decompile error: {}.".format(facts["failed"]))
    lines.append("**Observed.** Skipped indexes: {}.".format(facts["skipped"]))
    lines.append("**Observed.** Checkpoint next index: {}.".format(facts["next_index"]))
    lines.append("**Observed.** Elapsed time: {}.".format(facts["elapsed"]))
    if facts["partial"]:
        lines.append("**Observed.** This note is partial. The sweep has not reached the last function.")
    lines.append("")
    lines.append("## How to read the files")
    lines.append("")
    lines.append("Each line in `funcs/batch-<start>.jsonl.gz` is one function.")
    lines.append("The line has the address, the name, the callers, the callees, the strings, and the decompiled C text or an error.")
    lines.append("`names.tsv` has one row per function that was in the batch files.")
    lines.append("The columns are entry, old name, suggested name, label, rule, and evidence.")
    lines.append("`classes.md` groups functions by namespace or RTTI class.")
    lines.append("`network.md` lists type-index types whose names or UUIDs appear in a function.")
    lines.append("`strings.md` lists the {} strings that the most functions reference.".format(TOP_STRINGS))
    lines.append("Each string row shows up to {} function addresses. The count is the full function count.".format(STRING_FUNCTION_LIMIT))
    lines.append("`unknowns.md` groups functions whose label is Unknown.")
    lines.append("A label **Observed** means the name was taken from a string, a symbol, or RTTI in the program.")
    lines.append("A label **Inferred** means the name was taken from a thunk, a single callee, or callers of one class.")
    lines.append("A label **Unknown** means no rule supplied a name.")
    lines.append("")
    lines.append("## Counts by label")
    lines.append("")
    for label, count in sorted(facts["labels"].items()):
        lines.append("- {}: {}".format(label, count))
    lines.append("")
    lines.append("## Counts by rule")
    lines.append("")
    for rule, count in sorted(facts["rules"].items()):
        lines.append("- {}: {}".format(rule, count))
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def write_classes(path, classes):
    lines = ["# Classes", ""]
    lines.append("**Observed.** Class count: {}.".format(len(classes)))
    lines.append("A class comes from a function namespace or from an RTTI symbol.")
    lines.append("A suggested name supplies the class only when the function has no namespace and no RTTI class.")
    lines.append("")
    for name in sorted(classes):
        info = classes[name]
        lines.append("## {}".format(name))
        lines.append("")
        lines.append("**Observed.** Function count: {}.".format(len(info["methods"])))
        lines.append("**Observed.** Source: {}.".format(", ".join(sorted(info["sources"]))))
        if info["vtables"]:
            shown = ", ".join(sorted(info["vtables"])[:8])
            lines.append("**Observed.** Vtable addresses: {}.".format(shown))
        else:
            lines.append("**Unknown.** Vtable address.")
        lines.append("")
        lines.append("| entry | name | label |")
        lines.append("| --- | --- | --- |")
        for method in info["methods"]:
            lines.append("| {} | {} | {} |".format(method[0], method[1].replace("|", "/"), method[2]))
        lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def write_network(path, matches):
    lines = ["# Network types", ""]
    lines.append("**Observed.** Type table: `typeindex.json`.")
    lines.append("**Observed.** Names come from `uuid_to_class.json` when the UUID is in that catalog.")
    lines.append("**Observed.** Type count with at least one function: {}.".format(len(matches)))
    lines.append("**Inferred.** The role is handler, reader, or writer. The role comes from tokens in the matched name.")
    lines.append("")
    def sort_key(item):
        index = item[0][0]
        return (index is None, index if index is not None else 0, item[0][1])
    for (index, name), info in sorted(matches.items(), key=sort_key):
        index_text = "none" if index is None else str(index)
        lines.append("## {} ({})".format(name, index_text))
        lines.append("")
        if info["uuid"]:
            lines.append("**Observed.** UUID: {}.".format(info["uuid"]))
        else:
            lines.append("**Unknown.** Type index. The name matched a facet or replicated-state string.")
        lines.append("**Observed.** Function count: {}.".format(len(info["functions"])))
        lines.append("")
        lines.append("| entry | function | role |")
        lines.append("| --- | --- | --- |")
        for entry, function_name, role in info["functions"]:
            lines.append("| {} | {} | {} |".format(entry, function_name.replace("|", "/"), role))
        lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def write_strings(path, counter, users):
    lines = ["# Strings", ""]
    lines.append("**Observed.** Distinct strings: {}.".format(len(counter)))
    lines.append("**Observed.** The table keeps the {} strings with the highest function counts.".format(TOP_STRINGS))
    lines.append("**Observed.** Each row lists at most {} functions. The count column is the full count.".format(STRING_FUNCTION_LIMIT))
    lines.append("")
    lines.append("| count | string | functions |")
    lines.append("| --- | --- | --- |")
    for text, count in counter.most_common(TOP_STRINGS):
        shown = users[text][:STRING_FUNCTION_LIMIT]
        extra = count - len(shown)
        rendered = ", ".join(shown)
        if extra > 0:
            rendered = rendered + ", +{}".format(extra)
        safe = text.replace("|", "/").replace("\n", " ")
        lines.append("| {} | {} | {} |".format(count, safe, rendered))
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def write_unknowns(path, unknown_count, buckets, largest):
    lines = ["# Unknown functions", ""]
    lines.append("**Observed.** Unknown function count: {}.".format(unknown_count))
    lines.append("**Observed.** Address groups are {} bytes wide.".format(UNKNOWN_RANGE_BYTES))
    lines.append("")
    lines.append("## Groups")
    lines.append("")
    for key, count in sorted(buckets.items(), key=lambda item: (item[0] is None, item[0] or 0)):
        if key is None:
            lines.append("- other: {}".format(count))
        else:
            end = key + UNKNOWN_RANGE_BYTES - 1
            lines.append("- 0x{:x}-0x{:x}: {}".format(key, end, count))
    lines.append("")
    lines.append("## Largest")
    lines.append("")
    lines.append("**Observed.** The table lists the {} largest unknown functions in this note.".format(len(largest)))
    lines.append("")
    lines.append("| entry | size | old name | hint |")
    lines.append("| --- | --- | --- | --- |")
    for size, entry, old_name, hint in largest:
        safe_hint = hint.replace("|", "/").replace("\n", " ")
        lines.append("| {} | {} | {} | {} |".format(entry, size, old_name, safe_hint))
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def main():
    if len(sys.argv) != 2:
        print("usage: make_notes.py <outdir>", file=sys.stderr)
        return 2
    outdir = Path(sys.argv[1])
    meta = load_json(outdir / "meta.json")
    checkpoint = load_json(outdir / "checkpoint.json")
    run = load_json(outdir / "run.json")
    names = read_tsv(outdir / "names.tsv") if (outdir / "names.tsv").is_file() else []
    by_entry = {row["entry"]: row for row in names}
    labels = Counter(row["label"] for row in names)
    rules = Counter(row["rule"] for row in names)
    skipped_path = outdir / "skipped.txt"
    skipped = 0
    if skipped_path.is_file():
        skipped = sum(1 for line in skipped_path.read_text(encoding="utf-8").splitlines() if line.strip())

    catalog = TypeCatalog.load()
    classes = {}
    string_counts = Counter()
    string_users = defaultdict(list)
    network = {}
    unknown_count = 0
    buckets = Counter()
    largest = []
    written = 0
    decompiled = 0
    failed = 0

    for path in iter_batch_paths(outdir):
        for row in iter_records(path):
            written += 1
            if row.get("error"):
                failed += 1
            elif row.get("c"):
                decompiled += 1
            entry = row.get("entry") or ""
            name_row = by_entry.get(entry, {})
            suggested = name_row.get("suggested_name") or row.get("name") or ""
            label = name_row.get("label") or "Unknown"
            for class_name, source in class_keys(row, suggested):
                info = classes.setdefault(class_name, {"methods": [], "sources": set(), "vtables": set()})
                info["sources"].add(source)
                info["methods"].append((entry, suggested, label))
                for hit in row.get("vtables") or []:
                    vtable = hit.get("vtable") or ""
                    if vtable:
                        info["vtables"].add(vtable)
            for text in row.get("strings") or []:
                if not text:
                    continue
                string_counts[text] += 1
                if len(string_users[text]) < STRING_FUNCTION_LIMIT:
                    string_users[text].append(entry)
            blobs = list(row.get("strings") or [])
            if row.get("name"):
                blobs.append(row["name"])
            seen_types = set()
            for text in blobs:
                for match in catalog.match_text(text):
                    key = (match["type_index"], match["name"])
                    if key in seen_types:
                        continue
                    seen_types.add(key)
                    info = network.setdefault(key, {"uuid": match["uuid"], "functions": []})
                    info["functions"].append((entry, suggested, role_of(match["name"])))
            if label == LABEL_UNKNOWN:
                unknown_count += 1
                value = entry_int(entry)
                if value is None:
                    buckets[None] += 1
                else:
                    buckets[(value // UNKNOWN_RANGE_BYTES) * UNKNOWN_RANGE_BYTES] += 1
                size = int(row.get("size") or 0)
                hint = name_row.get("evidence") or ""
                largest.append((size, entry, row.get("name") or "", hint))

    largest.sort(reverse=True)
    largest = largest[:LARGEST_UNKNOWNS]

    started = parse_time(run.get("started_at") or meta.get("probed_at") or "")
    ended = parse_time(checkpoint.get("done_at") or "")
    elapsed = "Unknown"
    if started and ended:
        elapsed = format_duration((ended - started).total_seconds())
    next_index = int(checkpoint.get("next_index") or 0)
    total = int(checkpoint.get("total") or meta.get("function_count") or 0)
    facts = {
        "program_name": meta.get("program_name") or "Unknown",
        "image_base": meta.get("image_base") or "Unknown",
        "function_count": meta.get("function_count") if meta.get("function_count") is not None else "Unknown",
        "written": written,
        "decompiled": decompiled,
        "failed": failed,
        "skipped": skipped,
        "next_index": next_index,
        "elapsed": elapsed,
        "partial": total > 0 and next_index < total,
        "labels": labels,
        "rules": rules,
    }
    write_index(outdir / "index.md", facts)
    write_classes(outdir / "classes.md", classes)
    write_network(outdir / "network.md", network)
    write_strings(outdir / "strings.md", string_counts, string_users)
    write_unknowns(outdir / "unknowns.md", unknown_count, buckets, largest)
    print("wrote notes in {}".format(outdir))
    return 0


if __name__ == "__main__":
    sys.exit(main())
