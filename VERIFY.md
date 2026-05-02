# Verify release artifacts

This project is distributed as closed-source binaries. Verification focuses on artifact integrity, dependency-lock traceability, and release metadata consistency.

## 1. Verify SHA256 checksums

Download the target archive and `sha256sums.txt` from the same GitHub Release.

### PowerShell

```powershell
Get-FileHash .\codex-mimo-shim-vX.Y.Z-windows-x86_64.zip -Algorithm SHA256
```

Compare the result with `sha256sums.txt`.

### Git Bash / Linux / macOS

```bash
sha256sum -c sha256sums.txt
```

## 2. Verify Cargo.lock fingerprint

Download `Cargo.lock` and `Cargo.lock.sha256` from the release.

### PowerShell

```powershell
(Get-FileHash .\Cargo.lock -Algorithm SHA256).Hash.ToLower()
Get-Content .\Cargo.lock.sha256
```

The values should match.

### Git Bash / Linux / macOS

```bash
sha256sum Cargo.lock
cat Cargo.lock.sha256
```

## 3. Verify build metadata

Download the platform-specific build-info file from the release, for example `build-info-windows-x86_64.json`, `build-info-linux-x86_64.json`, or `build-info-macos-arm64.json`. It should contain fields similar to:

```json
{
  "name": "codex-mimo-shim",
  "version": "v0.1.6",
  "source_visibility": "private",
  "cargo_lock_sha256": "...",
  "release_workflow_sha256": "...",
  "public_build_rs_sha256": "...",
  "target": "x86_64-pc-windows-msvc",
  "profile": "release"
}
```

If the executable supports `--version` or `--build-info`, compare the embedded values with the matching build-info file:

```powershell
.\codex-mimo-shim.exe --build-info
```

or:

```powershell
.\codex-mimo-shim.exe --version
```

## 4. What this proves

These checks can show:

```text
- the artifact matches the published checksum;
- the published Cargo.lock matches its published SHA256;
- the executable declares the same dependency-lock fingerprint, if metadata output is integrated;
- the build metadata matches the official release workflow materials.
```

## 5. What this does not prove

These checks do not prove that closed-source software is attack-free. They provide tamper-evidence, dependency traceability, and release integrity checks.
