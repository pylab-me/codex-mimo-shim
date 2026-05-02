# Security Policy

## Supported versions

Only the latest release is actively supported unless otherwise stated in the release notes.

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub Security Advisories or by the maintainer contact method listed in the repository profile.

Do not open public issues for suspected vulnerabilities.

## Release integrity

Official binaries are published only through GitHub Releases in this public distribution repository.

Each release should include:

```text
- release zip
- sha256sums.txt
- Cargo.lock
- Cargo.lock.sha256
- build.rs
- build.rs.sha256
- release workflow copy or hash
- build-info.json
```

Users should verify checksums and metadata before running downloaded binaries.

## Closed-source model

`codex-mimo-shim` is currently distributed as closed-source binaries. This project does not claim that closed-source binaries can be independently proven attack-free. The release process is designed to make official artifacts identifiable, tamper-evident, and traceable to maintainer-controlled release workflows.
