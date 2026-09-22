#!/usr/bin/env python3
"""Suggest names for FUN_ functions in finished sweep batches."""

import sys
from pathlib import Path

from sweep_lib import TypeCatalog, apply_rules, load_compact, write_tsv


def main():
    if len(sys.argv) != 2:
        print("usage: resolve_unknowns.py <outdir>", file=sys.stderr)
        return 2
    outdir = Path(sys.argv[1])
    records = load_compact(outdir)
    catalog = TypeCatalog.load()
    rows = apply_rules(records, catalog)
    dest = outdir / "names.tsv"
    write_tsv(dest, rows)
    print("names {} functions -> {}".format(len(rows), dest))
    return 0


if __name__ == "__main__":
    sys.exit(main())
