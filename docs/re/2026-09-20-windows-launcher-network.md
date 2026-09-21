# Windows launcher: how the game can reach our Amazon-name mirror

Date: 2026-09-20. Build: Steam `25810808` (`BlightedNewWorld.exe` is a patched copy of
`NewWorld.exe`, PE machine `0x8664`). Labels: **Observed** (seen in code, data or a test),
**Inferred** (concluded from observed facts or from the AWS SDK for C++ source), **Unknown** (not
established).

## Summary for a non-programmer

- On Linux the launcher puts the game in a small private box (`bwrap`). Inside the box only, three
  Amazon names lead to our server and the game trusts our certificate for them
  (`2026-09-18-remote-config-layers.md`, section 3).
- Windows has no such box. Name lookups always use the one system hosts file and the system DNS.
  Certificate trust always uses the Windows certificate stores of the machine or of the user.
- The game itself has no setting, environment variable or config file that changes the three
  Amazon addresses, sends them through a proxy, or adds a trusted certificate.
- So no per-process route exists **without a change in the game process**. This note lists the
  options. **The choice is the user's.** No system-wide change is chosen, and nothing for Windows
  was built yet.

## 1. Evidence

### 1.1 The client's AWS HTTP clients

- **Observed.** The remote-config S3 client set-up `FUN_140853a80` (Ghidra sweep
  `local-data/re-sweep`) builds a default `Aws::Client::ClientConfiguration` (`FUN_147467a40`),
  then sets the region (`us-east-1`), the scheme (`1`, HTTPS), `verifySSL` (`1`), two 5,000 ms
  timeouts and a `DefaultRetryStrategy`. It does not set `endpointOverride`, `proxyHost`,
  `proxyPort` or a CA file.
- **Observed.** The executable has the RTTI names `WinHttpSyncHttpClient` and
  `WinINetSyncHttpClient` and imports `WINHTTP.dll` and `WININET.dll`. There is no
  `CurlHttpClient`.
- **Inferred (AWS SDK for C++ source).** With an empty `proxyHost`, both Windows HTTP clients of
  the SDK open their session with no proxy. A Windows proxy setting or a `HTTPS_PROXY` variable does
  not apply to them.
- **Inferred (Windows API).** WinHTTP and WinINet check the server certificate with the Windows
  certificate chain engine, which reads the machine and user stores. No API lets another process
  give a running process its own root list.

### 1.2 Environment variables and profile files

String search of `BlightedNewWorld.exe` (counts of matches):

| String | Count | Meaning |
|---|---|---|
| `AWS_ENDPOINT_URL` | 0 | The SDK is older than endpoint override by environment. |
| `endpoint_url`, `ignore_configured_endpoint_urls` | 0 | No endpoint override in the AWS profile file. |
| `AWS_CA_BUNDLE`, `ca_bundle` | 0 | No CA file by environment or profile (the SDK reads it only for curl). |
| `HTTPS_PROXY` | 0 | No SDK proxy variable. |
| `https_proxy`, `http_proxy`, `no_proxy` | 1 each | **Observed:** these belong to gRPC (`grpc/src/core/ext/filters/client_channel/http_proxy.cc` next to them), not to the AWS SDK. |
| `AWS_PROFILE`, `AWS_CONFIG_FILE`, `AWS_DEFAULT_REGION` | 1 each | The SDK reads a profile file, but that SDK version has no profile key for an endpoint, a proxy or a CA. |

### 1.3 The game folder

- **Observed.** `Bin64` holds `NewWorld.exe`, `BlightedNewWorld.exe` and `GameCrashUploader.exe`
  together. A DLL put in that folder to be loaded by name would also load into the retail
  `NewWorld.exe`.

## 2. Options

| # | Option | Per process? | What changes | Main risk |
|---|---|---|---|---|
| A | **Byte patch of the three AWS client set-ups** in `BlightedNewWorld.exe`, plus a **local proxy in the launcher**. The patch gives each client `proxyHost` `127.0.0.1` and the launcher's port, and `verifySSL` off. The launcher proxy accepts the game's TLS on loopback, then opens its own TLS connection to our server and checks our mirror certificate there (the same rustls check as on Linux). | Yes | Our patched exe only. No system setting. | The three clients accept any certificate. Their traffic goes only to loopback, so the exposure is to programs on the same PC. `docs/client-patch.md` already turns off one certificate check (DTLS) in the same copy. |
| B | **Renamed import + shim DLL.** Patch the import name `WINHTTP.dll` in `BlightedNewWorld.exe` to a DLL of our own that redirects name lookups and checks our certificate, then calls the real WinHTTP. | Yes | Our patched exe and one new DLL. | Our native code runs inside the game for every WinHTTP call; antivirus tools often flag such DLLs; more work than A. |
| C | Run the game in Windows Sandbox or a VM with its own hosts file and trust store. | Yes (a separate machine) | A VM. | Heavy; GPU and Steam ticket inside the VM are **Unknown**. |
| D | Edit the system hosts file and add our certificate to a Windows store. | **No** | Every program on the PC. | Forbidden by the task: retail play must stay untouched. Rejected. |
| E | DLL with a system name (for example `version.dll`) put next to the exe. | **No** | Also loads into `NewWorld.exe` (section 1.3). | Touches retail. Rejected. |
| F | Inject code into the running game (`CreateRemoteThread`, hooks). | Yes | Nothing on disk. | Acts like cheat software; most invasive. Rejected. |

**Recommendation: A.** It is the smallest change, it follows the existing client-patch method, it
keeps the certificate check at the launcher (pinned to our non-CA mirror certificate), and it
needs no system change.

### 2.1 Work that option A needs

1. Find the `ClientConfiguration` builders of the Kinesis and the STS clients (only the S3 one,
   `FUN_140853a80`, is known) and the offsets of `proxyHost`, `proxyPort` and `verifySSL` in that
   SDK version's `ClientConfiguration`. **Unknown** now.
2. A code cave per builder that writes the proxy host string and port, and clears `verifySSL`.
3. The launcher proxy (`cfg(windows)`): HTTP `CONNECT` on loopback for the three mirror names only,
   TLS to the game with a launcher-made certificate, TLS to the server with the mirror certificate
   pinned.
4. Windows game start: `BlightedNewWorld.exe` started directly by the launcher (never
   `NewWorld.exe`, never Steam's launcher, never Easy Anti-Cheat), with the server's arguments.
   Steam must run for the Steam ticket (as on Linux).
5. A live test on Windows.

## 3. Safety

- No option chosen here changes a system file, DNS, a certificate store or a Steam setting.
  Options D and E are rejected because they would change retail play.
- `NewWorld.exe` is never changed or started. Easy Anti-Cheat is never started or changed.
- Option A turns off certificate checks only in the three AWS clients of our patched copy, and
  only toward loopback. The launcher keeps the real check: it refuses a server certificate that is
  not the mirror certificate from `GET /launcher/info`, which itself must be a non-CA certificate
  valid for exactly the allowed names (`bins/launcher/src/mirror.rs`).
- The launcher proxy accepts only the three allowed name kinds (`is_aws_mirror_host`), so the game
  cannot use it to reach any other host.

## 4. Legal

- The patch is made by the player on the player's own installed copy, as the existing client patch
  (`docs/client-patch.md`). No game file is shipped.
- No Amazon service is contacted, signed for or impersonated toward anyone but the player's own
  game process. Our server answers only the requests that reach it.
- Facts come from our own static analysis of the user's installed client (Ghidra sweep, string
  search) and from public AWS SDK and Windows documentation. No third-party code was copied. Code
  is GPL-3.0-or-later (launcher).

## 5. Unknown

1. The AWS SDK for C++ version in the client (needed for the `ClientConfiguration` offsets).
2. The Kinesis and STS client builders.
3. Whether the SDK's WinHTTP client is the one used, or WinINet (both are present).
4. Whether `BlightedNewWorld.exe` starts on Windows without Steam's launcher (it does under
   Proton on Linux).
