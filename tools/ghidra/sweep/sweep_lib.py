"""Shared readers and name rules for the New World function sweep."""

import gzip
import json
import os
import re
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
# The client type table in the New World install (NW_GAME_DIR, as for content-tool).
TYPEINDEX_PATH = Path(os.environ.get("NW_GAME_DIR", "")) / "typeindex.json"
# The Aeternum-World type catalog, unpacked into the ignored local-data folder.
CATALOG_PATH = (
    REPO_ROOT / "local-data" / "Aeternum-World-main" / "Catalog" / "uuid_to_class.json"
)

LABEL_OBSERVED = "Observed"
LABEL_INFERRED = "Inferred"
LABEL_UNKNOWN = "Unknown"

RULE_STRING = "string_name"
RULE_VTABLE = "vtable_slot"
RULE_NETWORK = "network_type"
RULE_WRAPPER = "thunk_or_wrapper"
RULE_HELPER = "class_helper"
RULE_UNKNOWN = "unknown"
RULE_EXISTING = "existing_symbol"

MAX_PROPAGATION_PASSES = 10
MAX_HINT_STRINGS = 3
MAX_HINT_CALLERS = 3
EVIDENCE_MAX_CHARS = 500
MIN_SOURCE_STEM_CHARS = 4
GLOBAL_NAMESPACE = "Global"
SKIP_CLASS_ROOTS = frozenset({"std", "__gnu_cxx", "stdext", "operator", "__cxxabiv1"})
# AZStd strings name containers and allocators in asserts, never the function that holds them.
STRING_SKIP_ROOTS = SKIP_CLASS_ROOTS | frozenset({"AZStd"})
# A string that more functions than this reference does not identify one function.
MAX_NAMING_STRING_FUNCS = 10
FORMAT_MARK = "%"
# typeindex.json slot 0 is the all-zero UUID: the null type, not a message type.
NULL_UUID = "0" * 32
WRITER_TOKENS = ("Write", "Serialize", "Send", "Marshal")
READER_TOKENS = ("Read", "Parse", "Deserialize", "Recv", "Unmarshal")
TYPE_NAME_TEMPLATE = re.compile(
    r"\bGetTypeName<(?:class|struct) ([A-Za-z_][A-Za-z0-9_:]*)>"
)
PLACEHOLDER_NAME = re.compile(r"^(?:thunk_)?FUN_[0-9a-fA-F]+$")
QUALIFIED_NAME = re.compile(
    r"\b((?:[A-Za-z_][A-Za-z0-9_]*::)+[A-Za-z_~][A-Za-z0-9_]*)\b"
)
LOG_NAME = re.compile(
    r"\b([A-Z][A-Za-z0-9_]*) - ([A-Za-z_][A-Za-z0-9_]*)\b"
)
SOURCE_FILE = re.compile(
    r"([A-Za-z0-9_./\\:-]+)\.(?:cpp|cxx|cc|hpp|h|c)\b"
)
FACET_NAME = re.compile(
    r"\b([A-Za-z_][A-Za-z0-9_:]*(?:ClientFacet|ServerFacet)_[A-Za-z0-9_]+)\b"
)
REPLICATED_STATE = re.compile(
    r"\b([A-Za-z_][A-Za-z0-9_:]*ReplicatedState[A-Za-z0-9_:]*)\b"
)
HEX_UUID = re.compile(r"\b[0-9A-Fa-f]{32}\b")
DASHED_UUID = re.compile(
    r"\b[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}\b"
)
MSVC_RTTI = re.compile(r"\?AV([^@]+)@@")
RTTI_FOR = re.compile(r"typeinfo for (.+)", re.IGNORECASE)
RTTI_SUFFIX = re.compile(r"(.+)::(?:typeinfo|RTTI\w*)", re.IGNORECASE)
TOKEN = re.compile(r"[A-Za-z_][A-Za-z0-9_:.]{5,}")


def norm_uuid(value):
    return value.replace("-", "").upper()


def is_placeholder(name):
    return bool(name) and PLACEHOLDER_NAME.match(name) is not None


def clean_namespace(namespace):
    if not namespace:
        return ""
    text = namespace.strip()
    if text in (GLOBAL_NAMESPACE, "/"):
        return ""
    prefix = GLOBAL_NAMESPACE + "::"
    if text.startswith(prefix):
        text = text[len(prefix):]
    if text.startswith("/"):
        text = text[1:]
    if text == GLOBAL_NAMESPACE:
        return ""
    return text


def parse_rtti(text):
    if not text:
        return ""
    match = MSVC_RTTI.search(text)
    if match:
        return match.group(1)
    match = RTTI_FOR.search(text)
    if match:
        return match.group(1).strip()
    match = RTTI_SUFFIX.search(text)
    if match:
        return match.group(1).strip()
    return ""


def class_root(class_name):
    if not class_name:
        return ""
    return class_name.split("::", 1)[0]


def usable_qualified(name):
    if not name or is_placeholder(name):
        return False
    if class_root(name) in SKIP_CLASS_ROOTS:
        return False
    return True


class TypeCatalog:
    def __init__(self, slots, name_to_slot):
        self.slots = slots
        self.name_to_slot = name_to_slot
        self.by_uuid = {slot["uuid"]: slot for slot in slots if slot["uuid"] != NULL_UUID}
        grouped = {}
        for name, slot in name_to_slot.items():
            short = name.split("::")[-1]
            grouped.setdefault(short, []).append(slot)
        self.suffix_to_slot = {
            short: items[0] for short, items in grouped.items() if len(items) == 1
        }

    def slot_for_name(self, name):
        if name in self.name_to_slot:
            return self.name_to_slot[name]
        return self.suffix_to_slot.get(name)

    @staticmethod
    def load(typeindex_path=TYPEINDEX_PATH, catalog_path=CATALOG_PATH):
        with typeindex_path.open(encoding="utf-8") as handle:
            payload = json.load(handle)
        raw_slots = payload["typeIndex"]
        slots = {}
        ordered = []
        for index, uuid in enumerate(raw_slots):
            key = norm_uuid(uuid)
            slot = {"index": index, "uuid": key, "name": ""}
            slots[key] = slot
            ordered.append(slot)
        name_to_slot = {}
        if catalog_path.is_file():
            with catalog_path.open(encoding="utf-8") as handle:
                catalog = json.load(handle)
            for key, entry in catalog.get("entries", {}).items():
                slot = slots.get(norm_uuid(key))
                if slot is None:
                    continue
                if entry.get("is_empty_name"):
                    continue
                name = entry.get("name") or ""
                if not name:
                    continue
                slot["name"] = name
                name_to_slot[name] = slot
        return TypeCatalog(ordered, name_to_slot)

    @staticmethod
    def from_pairs(uuids, names_by_uuid):
        slots = []
        name_to_slot = {}
        for index, uuid in enumerate(uuids):
            key = norm_uuid(uuid)
            slot = {"index": index, "uuid": key, "name": names_by_uuid.get(key, "")}
            slots.append(slot)
            if slot["name"]:
                name_to_slot[slot["name"]] = slot
        return TypeCatalog(slots, name_to_slot)

    def slot_for_uuid(self, uuid):
        return self.by_uuid.get(norm_uuid(uuid))

    def match_text(self, text):
        if not text:
            return []
        found = []
        seen = set()

        def add(name, slot, kind):
            marker = (name, None if slot is None else slot["index"], kind)
            if marker in seen:
                return
            seen.add(marker)
            found.append({
                "name": name,
                "type_index": None if slot is None else slot["index"],
                "uuid": "" if slot is None else slot["uuid"],
                "kind": kind,
            })

        for match in DASHED_UUID.finditer(text):
            slot = self.slot_for_uuid(match.group(0))
            if slot is not None:
                label = slot["name"] or slot["uuid"]
                add(label, slot, "uuid")
        for match in HEX_UUID.finditer(text):
            slot = self.slot_for_uuid(match.group(0))
            if slot is not None:
                label = slot["name"] or slot["uuid"]
                add(label, slot, "uuid")
        direct = self.slot_for_name(text)
        if direct is not None:
            add(direct["name"] or text, direct, "catalog_name")
        for match in TOKEN.finditer(text):
            token = match.group(0)
            slot = self.slot_for_name(token)
            if slot is not None:
                add(slot["name"] or token, slot, "catalog_name")
        for match in FACET_NAME.finditer(text):
            name = match.group(1)
            slot = self.slot_for_name(name)
            add(slot["name"] if slot and slot["name"] else name, slot, "facet")
        for match in REPLICATED_STATE.finditer(text):
            name = match.group(1)
            slot = self.slot_for_name(name)
            add(slot["name"] if slot and slot["name"] else name, slot, "replicated_state")
        return found


def iter_batch_paths(outdir):
    folder = Path(outdir) / "funcs"
    if not folder.is_dir():
        return []
    paths = list(folder.glob("batch-*.jsonl.gz"))

    def sort_key(path):
        stem = path.name[len("batch-"):-len(".jsonl.gz")]
        return int(stem)

    return sorted(paths, key=sort_key)


def iter_records(path):
    handle = gzip.open(path, "rt", encoding="utf-8", errors="replace")
    try:
        for line in handle:
            text = line.strip()
            if not text:
                continue
            try:
                yield json.loads(text)
            except json.JSONDecodeError:
                continue
    except (EOFError, OSError, gzip.BadGzipFile):
        return
    finally:
        handle.close()


def compact_record(row):
    return {
        "index": row.get("index"),
        "entry": row.get("entry") or "",
        "name": row.get("name") or "",
        "namespace": row.get("namespace") or "",
        "size": int(row.get("size") or 0),
        "is_thunk": bool(row.get("is_thunk")),
        "thunk_target": row.get("thunk_target") or "",
        "thunk_target_name": row.get("thunk_target_name") or "",
        "is_external": bool(row.get("is_external")),
        "callers": list(row.get("callers") or []),
        "callees": list(row.get("callees") or []),
        "strings": list(row.get("strings") or []),
        "rtti": list(row.get("rtti") or []),
        "vtables": list(row.get("vtables") or []),
        "has_c": "c" in row and bool(row.get("c")),
        "error": row.get("error") or "",
    }


def load_compact(outdir):
    records = []
    for path in iter_batch_paths(outdir):
        for row in iter_records(path):
            records.append(compact_record(row))
    return records


def observed_class(record):
    namespace = clean_namespace(record.get("namespace") or "")
    if namespace:
        return namespace
    for hit in record.get("vtables") or []:
        parsed = parse_rtti(hit.get("symbol") or "")
        if parsed:
            return parsed
        namespace = clean_namespace(hit.get("namespace") or "")
        if namespace:
            return namespace
    for raw in record.get("rtti") or []:
        parsed = parse_rtti(raw)
        if parsed:
            return parsed
    return ""


def class_from_name(name):
    if not name or "::" not in name or is_placeholder(name):
        return ""
    return name.rsplit("::", 1)[0]


CAMEL_TOKEN = re.compile(r"[A-Z]+(?![a-z])|[A-Z]?[a-z]+|\d+")


def name_tokens(name):
    return set(CAMEL_TOKEN.findall(name.replace("_", " ").replace(":", " ")))


def role_of(name):
    tokens = name_tokens(name)
    if tokens.intersection(READER_TOKENS):
        return "reader"
    if tokens.intersection(WRITER_TOKENS):
        return "writer"
    return "handler"


def clip(text):
    text = " ".join(text.split())
    if len(text) > EVIDENCE_MAX_CHARS:
        return text[:EVIDENCE_MAX_CHARS]
    return text


def string_use_counts(records):
    counts = {}
    for record in records:
        for text in set(record.get("strings") or []):
            counts[text] = counts.get(text, 0) + 1
    return counts


def string_name(record, use_counts):
    for text in record.get("strings") or []:
        match = TYPE_NAME_TEMPLATE.search(text)
        if match and usable_qualified(match.group(1)):
            return match.group(1) + "::GetTypeName", clip(text)
        identifying = (
            FORMAT_MARK not in text
            and use_counts.get(text, 0) <= MAX_NAMING_STRING_FUNCS
        )
        match = QUALIFIED_NAME.search(text) if identifying else None
        if (
            match
            and usable_qualified(match.group(1))
            and class_root(match.group(1)) not in STRING_SKIP_ROOTS
        ):
            return match.group(1), clip(text)
        match = LOG_NAME.search(text) if identifying else None
        if match and match.group(1) not in SKIP_CLASS_ROOTS:
            suggestion = match.group(1) + "::" + match.group(2)
            return suggestion, clip(text)
        match = SOURCE_FILE.search(text)
        if match:
            stem = Path(match.group(1).replace("\\", "/")).name
            if len(stem) >= MIN_SOURCE_STEM_CHARS and stem not in SKIP_CLASS_ROOTS:
                return stem, "source file " + clip(text)
    return "", ""


def vtable_name(record):
    for hit in record.get("vtables") or []:
        class_name = parse_rtti(hit.get("symbol") or "")
        if not class_name:
            class_name = clean_namespace(hit.get("namespace") or "")
        slot_index = hit.get("slot_index")
        if not class_name or slot_index is None or int(slot_index) < 0:
            continue
        if class_root(class_name) in SKIP_CLASS_ROOTS:
            continue
        suggestion = class_name + "::vfunc_" + str(int(slot_index))
        evidence = "symbol " + (hit.get("symbol") or "") + " vtable " + (hit.get("vtable") or "")
        return suggestion, clip(evidence)
    return "", ""


def network_name(record, catalog):
    blobs = list(record.get("strings") or [])
    if record.get("name"):
        blobs.append(record["name"])
    if record.get("namespace"):
        blobs.append(record["namespace"])
    best = None
    best_len = -1
    evidence_text = ""
    for text in blobs:
        for match in catalog.match_text(text):
            if len(match["name"]) > best_len:
                best = match
                best_len = len(match["name"])
                evidence_text = text
    if best is None:
        return "", ""
    role = role_of(best["name"])
    index_text = "none" if best["type_index"] is None else str(best["type_index"])
    evidence = (
        "type " + best["name"]
        + " index " + index_text
        + " kind " + best["kind"]
        + " role " + role
        + " text " + evidence_text
    )
    return best["name"], clip(evidence)


def callee_display(entry, by_entry, fallback_name):
    if fallback_name and not is_placeholder(fallback_name):
        return fallback_name
    other = by_entry.get(entry)
    if other is None:
        return entry or fallback_name or ""
    name = other.get("name") or ""
    if name:
        return name
    return entry


def apply_rules(records, catalog):
    by_entry = {record["entry"]: record for record in records}
    use_counts = string_use_counts(records)
    rows = {}

    def put(record, suggested, label, rule, evidence):
        rows[record["entry"]] = {
            "entry": record["entry"],
            "old_name": record["name"],
            "suggested_name": suggested,
            "label": label,
            "rule": rule,
            "evidence": clip(evidence),
        }

    for record in records:
        if not is_placeholder(record["name"]):
            namespace = clean_namespace(record.get("namespace") or "")
            evidence = "symbol"
            if namespace:
                evidence = "namespace " + namespace
            put(record, record["name"], LABEL_OBSERVED, RULE_EXISTING, evidence)
            continue
        suggested, evidence = string_name(record, use_counts)
        if suggested:
            put(record, suggested, LABEL_OBSERVED, RULE_STRING, evidence)
            continue
        suggested, evidence = vtable_name(record)
        if suggested:
            put(record, suggested, LABEL_OBSERVED, RULE_VTABLE, evidence)
            continue
        suggested, evidence = network_name(record, catalog)
        if suggested:
            put(record, suggested, LABEL_OBSERVED, RULE_NETWORK, evidence)

    for record in records:
        if record["entry"] in rows:
            continue
        if record.get("is_thunk") or record["name"].startswith("thunk_FUN_"):
            target_name = record.get("thunk_target_name") or ""
            display = callee_display(record.get("thunk_target") or "", by_entry, target_name)
            if not display and record.get("callees"):
                display = callee_display(record["callees"][0], by_entry, "")
            if display:
                put(
                    record,
                    display + "_thunk",
                    LABEL_INFERRED,
                    RULE_WRAPPER,
                    "thunk target " + (record.get("thunk_target") or display),
                )
            continue
        callees = record.get("callees") or []
        if len(callees) == 1:
            display = callee_display(callees[0], by_entry, "")
            if display:
                put(
                    record,
                    display + "_wrapper",
                    LABEL_INFERRED,
                    RULE_WRAPPER,
                    "one callee " + callees[0],
                )

    def current_name(record):
        row = rows.get(record["entry"])
        if row:
            return row["suggested_name"]
        return record["name"]

    def current_class(record):
        observed = observed_class(record)
        if observed:
            return observed
        return class_from_name(current_name(record))

    for _pass in range(MAX_PROPAGATION_PASSES):
        pending = []
        for record in records:
            if record["entry"] in rows:
                continue
            callers = record.get("callers") or []
            if not callers:
                continue
            classes = set()
            blocked = False
            for caller_entry in callers:
                caller = by_entry.get(caller_entry)
                if caller is None:
                    blocked = True
                    break
                class_name = current_class(caller)
                if not class_name:
                    blocked = True
                    break
                classes.add(class_name)
            if blocked or len(classes) != 1:
                continue
            class_name = next(iter(classes))
            token = record["entry"]
            if token.startswith("0x"):
                token = token[2:]
            pending.append((record, class_name + "::helper_" + token, callers[0]))
        if not pending:
            break
        for record, suggested, caller_entry in pending:
            put(
                record,
                suggested,
                LABEL_INFERRED,
                RULE_HELPER,
                "callers only from one class, example " + caller_entry,
            )

    for record in records:
        if record["entry"] in rows:
            continue
        hints = []
        for text in (record.get("strings") or [])[:MAX_HINT_STRINGS]:
            hints.append("string " + text)
        for caller_entry in (record.get("callers") or [])[:MAX_HINT_CALLERS]:
            caller = by_entry.get(caller_entry)
            if caller is None:
                hints.append("caller " + caller_entry)
            else:
                hints.append("caller " + current_name(caller) + " " + caller_entry)
        evidence = "; ".join(hints) if hints else "no string and no caller"
        put(record, record["name"], LABEL_UNKNOWN, RULE_UNKNOWN, evidence)

    ordered = []
    for record in records:
        ordered.append(rows[record["entry"]])
    return ordered


def write_tsv(path, rows):
    path = Path(path)
    tmp = path.with_suffix(path.suffix + ".tmp")
    with tmp.open("w", encoding="utf-8", newline="\n") as handle:
        handle.write("entry\told_name\tsuggested_name\tlabel\trule\tevidence\n")
        for row in rows:
            fields = [
                row["entry"],
                row["old_name"],
                row["suggested_name"],
                row["label"],
                row["rule"],
                row["evidence"],
            ]
            handle.write("\t".join(field.replace("\t", " ").replace("\n", " ") for field in fields))
            handle.write("\n")
    tmp.replace(path)


def read_tsv(path):
    rows = []
    with Path(path).open(encoding="utf-8") as handle:
        header = handle.readline()
        if not header:
            return rows
        for line in handle:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 6:
                continue
            rows.append({
                "entry": parts[0],
                "old_name": parts[1],
                "suggested_name": parts[2],
                "label": parts[3],
                "rule": parts[4],
                "evidence": parts[5],
            })
    return rows
