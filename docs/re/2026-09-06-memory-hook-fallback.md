# Backup plan: in-memory client hook for world entry

Date: 2026-09-06.

This note records a fallback. It is not the primary plan. Do not build it
unless the primary plan fails.

## Primary plan (preferred)

Select the client's built-in plain-TCP transport (`TransportLayerLibUV`).
That path receives no embedded certificate, so it avoids the
`gridmate-udp` DTLS certificate trust. See
[world-entry alternate transport](2026-09-06-world-entry-alternate-transport.md)
and [launcher and certificate trust](2026-08-31-launcher-and-trust.md).

## Why a fallback is needed

The primary plan works only if two facts hold:

1. The `transport-type` value can be set from `@assets@/ClientOverride.json`
   without a binary patch.
2. The TCP path performs no later certificate or security check.

If either fact fails, use the backup below.

## Backup: in-memory certificate hook

Source of the idea: the Azoth catalog owner, relayed by the user on
2026-09-06. The exact words: "there is no need of certificates. you just
need to apply a proxy in the client memory. this is not the right way but
is the best way." This is an outside claim, not local validation.

Idea: do not change the certificate on the server. Change the client's
trust decision at run time, in memory. Then the real client connects to
the existing `rep-gateway` over the normal `gridmate-udp` DTLS path, which
already works with the project test client.

Two forms of the hook:

- Force the client to accept any server certificate. The secure-context
  builder sets verify with a null callback and uses
  `SSL_CTX_set_verify` (old build `FUN_1478eff50`, new build
  `FUN_1478f1590`, body unchanged). A hook can set the verify mode to
  none, or make the verify callback always return success.
- Replace the trusted certificate in memory with the gateway's own
  certificate before the secure context is built. The embedded PEM is
  parsed by the `gridmate-udp` path (protocol note section 1.4).

The existing gateway certificate code is
[crates/nw-dtls/src/lib.rs](../../crates/nw-dtls/src/lib.rs).

## Risks and open items for the backup

- The hook runs at run time and must be re-applied on each launch.
- It can break on each client update.
- Easy Anti-Cheat is not studied. A hook while EAC is active is likely
  blocked or flagged. The client must launch without EAC. This is an
  open item for every client-side route, the primary plan included.
- No injector or hook code exists in this repository yet.

## Status

Backup only. The chosen path is the TCP transport. Build the hook only if
the TCP transport path cannot be made to work.
