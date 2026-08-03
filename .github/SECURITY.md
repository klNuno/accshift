# Security Policy

## Supported versions

Only the latest release line receives security fixes. Older versions are not
patched; upgrade to the current release before reporting.

| Version | Supported |
| ------- | --------- |
| 1.0.x   | Yes       |
| < 1.0   | No        |

## Reporting a vulnerability

Report privately through GitHub Security Advisories:

**https://github.com/klNuno/accshift/security/advisories/new**

Do not open a public issue, discussion or pull request for a vulnerability.

Include the accshift version, your operating system, the affected platform
integration if any, and the steps needed to reproduce. Expect a first response
within a few days. Do not include real session tokens, cookies or credentials in
the report; redact them or describe their shape instead.

## Scope

In scope:

- Handling of session tokens, cookies and captured session snapshots at rest,
  including the encryption backends (DPAPI on Windows, Secret Service on Linux,
  Keychain on macOS) and the `com.accshift.desktop` keyring service entries.
- Credential and secret handling in the config, logs and migration backups,
  including anything that ends up unredacted in a log file.
- The exclusive file lock shared by the GUI and CLI on mutating operations.
- The `accshift://` deep link handler and any input reaching it.
- The updater and its signature verification.
- The webview-reachable command surface and input validation on platform ids.
- The optional CS2 stats bridge and the storage of its Bearer token.

Out of scope:

- Vulnerabilities in the game launchers or their services (Steam, Riot Games,
  Battle.net, Epic Games, Ubisoft Connect, Roblox, GOG Galaxy, Jagex Launcher,
  Discord). Report those to the vendor.
- Social engineering, phishing, or physical access attacks.
- Anything that requires an already compromised machine, an attacker with an
  interactive session as the same user, or malware already running with the
  user's privileges. accshift binds secrets to the OS user session, so an
  attacker who is already that user is outside the threat model.
- Missing hardening that has no demonstrated impact, and automated scanner
  output without a working proof of concept.

## Security model

accshift never asks for or stores passwords. It snapshots the session state the
launcher already keeps on disk, so switching accounts restores a local session
rather than reauthenticating. Sensitive material (Steam Web API key, Roblox
login cookie, session snapshots for Riot Games, Ubisoft Connect, Epic Games, GOG
Galaxy, Jagex Launcher and Discord, and the optional CS2 bridge token) is
encrypted at rest with OS-backed protection: DPAPI on Windows, Secret Service
via the `keyring` crate on Linux, Keychain on macOS. Snapshots are decrypted
only while staging or restoring the selected session. Secrets are bound to your
OS user session, so copying the config to another machine or user does not yield
decryptable secrets. Battle.net does not use the snapshot mechanism, and the CS2
bridge URL itself is stored in plain config, so a secret embedded in that URL is
not vault-protected.

Snapshots captured before encryption shipped carry no header and were read back
in the clear. Capturing an account again encrypts it, but that only happens when
you switch away from it, so an account left untouched since the upgrade kept its
session material unencrypted. The app now sweeps its own snapshot store once per
launch, in the background, and rewrites any headerless file encrypted. The
rewrite stages a temporary file and renames it into place, and a file that
cannot be encrypted is left exactly as it was rather than deleted.

### What the PIN lock does, and what it does not

The optional 4-digit PIN is an access gate on the app and the CLI, not a
cryptographic boundary. It is stored as a PBKDF2-HMAC-SHA256 hash (100 000
iterations, 16-byte random salt, 32-byte output) and checked in constant time,
so the hash in the settings file does not reveal the PIN. That is the whole of
its job.

It derives no key and encrypts nothing. Session material is protected by the OS
backends listed above, which are bound to your OS user session and not to the
PIN. Someone already running code as your OS user therefore decrypts snapshots
without ever knowing the PIN, exactly as the out-of-scope list above states.

So the PIN buys resistance to someone reaching your unlocked machine, and
nothing beyond that. It is not a second factor and it is not a vault password.
Enable it for a shared desk; do not treat it as protection against malware or
against another process running as you.

Accounts and settings stay on the machine. Outbound traffic is limited to
launcher operations you start, optional account lookups and health checks, the
optional CS2 bridge, update checks, and anonymous usage counters. Those counters
never carry account names or ids, credentials, file paths or local file
contents. They are on after the first-launch screen and off in one click in
Settings, Privacy; the enhanced tier is a separate opt-in.

Full detail on what is collected: [analytics](../docs/analytics.md), in this
repository so that `git log` shows every change to it. See also
[Security & Data](https://github.com/klNuno/accshift/wiki/Security) and the
[Privacy Policy](https://github.com/klNuno/accshift/wiki/Privacy-Policy) on the
wiki.
