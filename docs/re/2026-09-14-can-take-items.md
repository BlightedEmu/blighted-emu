# CanTakeItems: the client check behind "Invalid parameters"

Date: 2026-09-14. Build: Steam `25810808`. Labels: **Observed**, **Inferred**, **Unknown**.

Sources: the Ghidra project `<re-scratch>/ghidra-25810808` (analyzed `NewWorld.exe`,
Ghidra 12.1.4, headless Java scripts, see section 5), the client `Game.log` of the live run of
2026-09-14 (`local-data/logs/run-2026-09-14-1233/Game.log`), and the retail ledger
`local-data/captures/own/newchar-linux-2026-09-12/ledger.bin`.

## 1. The failure

**Observed.** Every interaction with the beach bag in the live run logged
`PlayerTradeComponent - CanTakeItems - Invalid parameters.` and the client never sent the
`PlayerTradeComponentServerFacet_TakeAllItems` message (type 2939). The interaction itself was
resolved: the client message carried the same kind (28) and the bag's `GdeRef` as retail.

## 2. `CanTakeItems` (`FUN_142d7ae90`)

**Observed** (decompile, `local-data/tools/ghidra/can-take-items.txt`). The log fires when the
call comes in with a bad container id or nothing to take:

```c
if ((*param_2 == 0xffffffff) || (*param_3 == param_3[1])) {
    FUN_141598f70(param_1, "PlayerTradeComponent - CanTakeItems - Invalid parameters.");
}
```

`param_2` is the container entity id; `param_3` is a vector of 0x28-byte item records, empty
when begin equals end. The check is fully client-side: no server message is needed before the
client decides and drops the request.

## 3. The callers

**Observed** (decompile, `local-data/tools/ghidra/can-take-items-callers.txt`). Three functions
call `CanTakeItems`; the take-all path is `FUN_142d96970`:

```c
plVar3 = FUN_142d875d0(param_1, param_2);            // resolve the container component
if (plVar3 != NULL) {
    uVar4 = (**(code **)(*plVar3 + 0x118))(plVar3); // its items vector
    iVar2 = FUN_142d7ae90(param_1, param_2, uVar4, ...);
}
```

The resolver `FUN_142d875d0` walks the entity's components for the interface id
`{91B8850B-3BDF-1D1F-16BB-4B7CB8DDC4E1}` (the container interface; three component classes
implement it) and logs its own errors: "unknown Container EntityId", "no container component",
"invalid container". None of those appear in the live `Game.log`, so the bag's container
component resolved and the failure was the empty items vector.

The other callers are a single-item take (`FUN_142d96fd0`, builds the vector from the passed
item) and `FUN_142d97810`.

## 4. Where the items come from

**Observed.** The bag's replicated card carries no item state: `LootDropReplicatedState`
(type 2027) is only `m_rarityLevel` (the glow). Between the bag card (t=164262) and the take
(t=169077) retail sends only own-position bundles. The item list is not client-slice data
either. At t=164578 (~316 ms after the card) the retail server emits on the bag's group:

- `c2/t1755` `MB::ContainerComponentReplicatedState`: list version 5, the three archetype items
  (`syncReason` 3, container slots 0 to 2, counters 0 to 2, gear score 75, bound, 16-byte instance
  ids), `bonusMaxEncumbrance` 0, `wasEmptied` false;
- `c6/t2027` body `010100000001` (glow on).

At the take (t=169112) it empties the container (version 6, three zeroed records with
`syncReason` 6, `wasEmptied` true), sets `c3/t2930` `010100`, `c6/t2027` off, the two delayed
events (`c7`/`c8` `MB::DelayedEventComponentReplicatedState`), and clears the player's
interaction target (`c26/t3752` on the own group, body `01 01` + the bag's entity id). At
t=169143 the player's `c61/t1755` delta adds the items at bag slots 6 to 8, instance ids
unchanged.

**Observed.** `VInitialContainerContents` exists as a type name in the binary; how the retail
server builds the bag's first list is **Unknown** (the loot table is `FTUE_ArchetypesLoot1`).

**Unknown.** The wire perk index: the two Destroyer weapons carry perk `1203`, but no
enumeration of the `perk_data` datasheets reproduces it (sorted/unsorted sheet orders and row
positions all miss). Records go out without perks until the rule is known.

## 5. The Ghidra setup

**Observed.** The project `<re-scratch>/ghidra-25810808/NewWorld25810808.gpr` holds an analyzed
`NewWorld.exe` of build `25810808`. Ghidra 12.1.4 runs it headless; the Java scripts are in
`local-data/tools/ghidra/scripts/` (find a string's xrefs and decompile the owners, decompile
at an address, dump bytes, resolve a vtable's owner class). Example:

```
$GHIDRA/support/analyzeHeadless <re-scratch>/ghidra-25810808 NewWorld25810808 \
    -process NewWorld.exe -noanalysis -scriptPath local-data/tools/ghidra/scripts \
    -postScript FindAndDecompile.java "PlayerTradeComponent - CanTakeItems" out.txt
```

Jython is not installed in this Ghidra; Java scripts work.

## 6. Consequence

The sim sends the bag's `c2/1755` fill and the take group itself
(`bins/sim/src/world/statics.rs`: `bag_loot`, `bag_emptied`, `interactor_cleared`; driven from
`Order::ShowIsland` and the `TakeAllItems` arm of `facet_message`). The bodies are
byte-verified against the capture; the perk slots stay 0 (section 4).
