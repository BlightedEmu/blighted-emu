# Local sources for world entry

Date: 2026-09-06.

This is a read-only source review. No client or server was changed.
No packet capture was replayed. No packet bodies or private ids are included.

## Most useful new lead

`local-data/azoth-catalog/packets/1635.json` marks LevelInfoChangedMsg
as solved, has_spec true, spec_confidence inferred.
It describes the level name, coatlicue path, phasing origin and bounds,
phase entries, client context, master-player flag, and instance stamp.
It says the client loads the named map and uses the context in later
state bundles. This is an outside project's claim, not local validation.

The older `docs/re/2026-09-04-bootstrap-message-bodies.md` leaves this
wire layout unknown. The archive is a concrete candidate grammar to test
against the local captures and native reader.

Do not implement the displayed field table literally:
- Several offsets refer to one sample and change with string lengths.
- The displayed clientCtx row says u8 but includes examples above 255.
- Some names occur twice, once with examples and once with descriptions.
- Prefix integers are called vlq32. The emulator has a specific prefix
  codec; the label alone does not establish which encoding is meant.

## Still open

`local-data/azoth-catalog/packets/1628.json` marks
PlayerManagerSelfIdentificationMsg captured, has_spec false, with no
field list. That file does not finish the identity message.

The existing local notes identify the reader FUN_1414fd530 and handler
FUN_146454c00 in the old client. They record 12,702-byte bodies in 43
contexts, and a partly decoded constant tail. Finish the reader tree;
do not copy a captured character's identity or session bytes into the
emulator.

## First Light archive

`local-data/first-light-main/server/javelin/self_ident.py` explicitly
calls its 21-byte-minimum encoder a hypothesis from in-memory fields.
The local capture-backed notes reject that as the observed body form.
`level_info_changed.py` also labels its format a hypothesis and assumes
little-endian integers. It is not a validated replacement for our wire
reader. Its self-round-trip tests cannot establish game compatibility.

The state_10_unblock_synthesis.md also uses an old/misidentified
SelfIdentification type. Our post-registration sequence note corrects
those first-light assignments. Use the class UUID and native reader to
bind a message, not an old packet nickname.

Read the archive for leads, not as a working server implementation.

## Practical order

1. Finish the alternate transport/config-selection investigation for the
   certificate block. None of these packet specifications fixes DTLS trust.
2. Validate the archived 1635 field list against our local captures.
3. Finish 1628 from its native reader and identity handler.
4. Check context and entity ids across both messages and state bundles.
5. Test actual client state transitions through map load and player spawn.

Source paths above are git-ignored local resources and may not exist on
another checkout. No outside source code was copied.
