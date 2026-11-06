# Export report

Prepared 2026-11-05 for manual upload. No Git commands, Git initialization, repository connection, uploads, automated tests, benchmarks or compilation were performed. All writes and capture-session settings were confined to this GitHub folder; original source/resource hashes were checked unchanged. The temporary documentation application session was closed and its synthetic log, settings and diagnostics removed.

## Included release

- Windows x64, DingoTail 0.2.0: builds/windows-x64/dingotail.exe (12,423,168 bytes).
- SHA-256: c1a4d0afbd4a9404b5e9f7c4f72ea8668b72add51aae761309407ef681ea4a7f.
- Copied from the existing target/release executable without rebuilding; byte-identical to the existing QA release-smoke executable. PE header identifies AMD64. Current Rust sources, Cargo manifests/lockfile and build.rs match the recorded source-correspondence hashes.
- Runtime branding/fonts are embedded. Existing project notices are included. Native OS graphics/system components remain external prerequisites.
- No Linux/macOS release binaries were found. Older dist executables and archives were excluded.

## Exclusions and scope

Excluded target intermediates, debug builds, QA environments and settings, private server information, integration fixtures, standalone test files, backups, caches, logs, performance tooling, development reports and editor metadata. The old archives include obsolete material and are not copied wholesale.

Application Rust files are byte-identical to their originals. Embedded cfg(test) blocks and references to omitted test-only modules remain to avoid editing application code. Normal compilation does not enable them; cargo test is not supported by this reduced export. This is the explicit exception to removing all test-related content.

Only exported packaging/documentation was adapted: removed required QA-report/fixture inputs and profiling-tool references, included screenshot resources in packages, and removed the workflow's regression-test step because standalone tests are excluded. Package scripts were inspected statically, not executed. Cargo.toml and Cargo.lock remain unchanged, including the dev dependency entry.

No obvious credentials/private keys or private machine paths were found in exported text. Private infrastructure references found in excluded test/report material were not copied. This is a static screening, not a guarantee that every possible secret representation has been identified.

## Screenshots

All three images were captured from the existing production executable on the available Windows 11 QA desktop using software rendering and an isolated settings directory. They show safe synthetic local data, not live infrastructure. Captures preserve the entire maximized window, uncropped, at 1920 × 1032 (available work area); no mockups, retouching or fabricated UI.

- local-log-highlighting.jpg: local file reader, severity colors and follow controls.
- search-filters.jpg: include filter, matching messages and visible/retained counts.
- highlighting-settings.jpg: ordered rules, colors and preview.

Remote SSH, SFTP and journal features are documented from source and existing documentation; no new remote connection was made for screenshots.

## Static inspection

Verified original selected file hashes unchanged; all exported application files unchanged; binary hash/architecture and prior source correspondence; required embedded assets; manifest and lockfile presence; README relative links and screenshots; packaging Python syntax through parsing only; absence of .git, capture profiles, logs, caches and standalone test files. No build/test verification was run.

Dependency sources are not vendored in this minimal source tree: building needs registry access or a populated Cargo cache. The future release packager vendors locked dependencies, but it was not run. Linux/macOS native compatibility, signing and full Windows validation remain outstanding. Existing license, copyright and branding terms were preserved; no license was invented.

## Follow-up cleanup and review

Removed the three unused root reference images and assets/brand/favicon-48x48.png, which was unused and byte-identical to icon-48.png. Original project files remain untouched. Updated branding descriptions and notice preambles to match the export contents. The source packager no longer collects arbitrary root PNGs.

Retained artwork masters and provenance for maintaining the shipped brand; runtime/platform icons for builds and packaging; license notices for distribution; screenshots and this requested export report for review; and the requested Windows release. No other standalone caches, settings, logs, backups, credential files or obsolete builds were found. Embedded test/QA code remains in unchanged Rust modules as documented above.

Credential review found no apparent real API keys, passwords or private keys. Authentication code and the dummy unit-test value "secret" are not saved credentials. Known token formats and sensitive file names were screened, including binary bytes. Static screening cannot guarantee detection of arbitrary or encoded secrets.

## Complete directory tree

```text
GitHub/
├── .github/
│   └── workflows/
│       └── packages.yml
├── assets/
│   ├── brand/
│   │   ├── provenance/
│   │   │   ├── lockup.txt
│   │   │   ├── mascot.txt
│   │   │   └── wordmark.txt
│   │   ├── BRANDING.md
│   │   ├── dingotail.ico
│   │   ├── icon-128.png
│   │   ├── icon-16.png
│   │   ├── icon-24.png
│   │   ├── icon-256.png
│   │   ├── icon-32.png
│   │   ├── icon-48.png
│   │   ├── icon-64.png
│   │   ├── lockup.png
│   │   ├── mascot-master.png
│   │   ├── mascot.png
│   │   ├── README.md
│   │   ├── wordmark-master.png
│   │   └── wordmark.png
│   ├── icons/
│   │   ├── MaterialIconsOutlined-Regular.codepoints
│   │   └── MaterialIconsOutlined-Regular.otf
│   └── windows.rc
├── builds/
│   └── windows-x64/
│       └── dingotail.exe
├── docs/
│   ├── screenshots/
│   │   ├── highlighting-settings.jpg
│   │   ├── local-log-highlighting.jpg
│   │   └── search-filters.jpg
│   └── EXPORT.md
├── licenses/
│   ├── DEPENDENCIES.json
│   ├── DEPENDENCY-NOTICES.txt
│   ├── Material-Icons-LICENSE.txt
│   └── THIRD-PARTY-NOTICES.txt
├── packaging/
│   ├── dingotail.desktop
│   └── dingotail.spec
├── src/
│   ├── ui/
│   │   └── editors.rs
│   ├── credentials.rs
│   ├── local.rs
│   ├── main.rs
│   ├── model.rs
│   ├── network.rs
│   ├── platform.rs
│   ├── rendering.rs
│   ├── theme.rs
│   └── ui.rs
├── tools/
│   └── package_release.py
├── .gitignore
├── BRANDING.md
├── build.rs
├── Cargo.lock
├── Cargo.toml
├── COPYRIGHT
├── LICENSE
├── package.ps1
├── PORTABILITY.md
├── README.md
└── SHA256SUMS.txt
```
