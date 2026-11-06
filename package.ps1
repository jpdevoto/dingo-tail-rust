# Copyright (C) 2026 Juan Pablo Devoto
# SPDX-License-Identifier: GPL-3.0-only
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed' }
    python tools/package_release.py --target windows-x64 --binary target/release/dingotail.exe
    if ($LASTEXITCODE -ne 0) { throw 'Source/license packaging failed' }
} finally { Pop-Location }

