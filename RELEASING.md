# Releasing accshift

This project currently ships Windows binaries only (`nsis` and `msi`).
Auto-update and installer/update UX can be added later.

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
- generate `SHA256SUMS.txt`
- create a **draft** GitHub release with assets attached

## 6. Publish
Open the draft release on GitHub, review notes/assets, then publish.
