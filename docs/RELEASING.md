# Releasing accshift

This project currently ships Windows binaries only (`nsis` and `msi`).

## 0. One-time updater key setup
Generate a signing keypair once (keep the private key secret forever):

```powershell
pnpm tauri signer generate --ci -w "$env:USERPROFILE\.tauri\accshift.key"
```

Then add GitHub repository secrets:
- `TAURI_SIGNING_PRIVATE_KEY` = full content of `C:\Users\<you>\.tauri\accshift.key`
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = private key password (optional; empty if key has no password)

## 1. Update version
Use the same version in all files:
- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`

Example: `0.2.0`

## 2. Commit and push
Commit the version bump and push to `main`.

## 3. Wait for CI
Ensure the `CI` workflow is green.

## 4. Create and push tag
Tag format must be `vX.Y.Z` and must match file versions.

Example:
```powershell
git tag v0.2.0
git push origin v0.2.0
```

## 5. Release workflow output
The `Release` workflow will:
- build Tauri bundles for Windows
- produce `nsis` (`.exe`) and `msi` (`.msi`) artifacts
- sign updater artifacts (`.sig`) using `TAURI_SIGNING_PRIVATE_KEY`
- generate `latest.json` for auto-update
- generate `SHA256SUMS.txt`
- create a **draft** GitHub release with assets attached

## 6. Publish
Open the draft release on GitHub, review notes/assets, then publish.

Auto-update endpoint used by the app:
- `https://github.com/klNuno/accshift/releases/latest/download/latest.json`
