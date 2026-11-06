# Release builds

Version 0.2.0 installers and executables, copied unchanged from the published release assets. Linux and macOS builds have compiled successfully but have not been desktop-tested. macOS bundles are ad-hoc signed, not Developer ID signed or notarized.

| Platform | Binary | Checksums | Matching source archive |
| --- | --- | --- | --- |
| windows-x64 | [DingoTail-0.2.0-windows-x64.exe](windows-x64/DingoTail-0.2.0-windows-x64.exe) | [SHA-256](windows-x64/DingoTail-0.2.0-windows-x64-SHA256SUMS.txt) | [Repository source](../src/) |
| linux-deb-x64 | [DingoTail-0.2.0-linux-deb-x64.deb](linux-deb-x64/DingoTail-0.2.0-linux-deb-x64.deb) | [SHA-256](linux-deb-x64/DingoTail-0.2.0-linux-deb-x64-SHA256SUMS.txt) | [DingoTail-0.2.0-linux-deb-x64-corresponding-source.zip](https://github.com/jpdevoto/dingo-tail-rust/releases/download/v0.2.0/DingoTail-0.2.0-linux-deb-x64-corresponding-source.zip) |
| linux-rpm-x64 | [DingoTail-0.2.0-linux-rpm-x64.rpm](linux-rpm-x64/DingoTail-0.2.0-linux-rpm-x64.rpm) | [SHA-256](linux-rpm-x64/DingoTail-0.2.0-linux-rpm-x64-SHA256SUMS.txt) | [DingoTail-0.2.0-linux-rpm-x64-corresponding-source.zip](https://github.com/jpdevoto/dingo-tail-rust/releases/download/v0.2.0/DingoTail-0.2.0-linux-rpm-x64-corresponding-source.zip) |
| macos-x64 | [DingoTail-0.2.0-macos-x64.dmg](macos-x64/DingoTail-0.2.0-macos-x64.dmg) | [SHA-256](macos-x64/DingoTail-0.2.0-macos-x64-SHA256SUMS.txt) | [DingoTail-0.2.0-macos-x64-corresponding-source.zip](https://github.com/jpdevoto/dingo-tail-rust/releases/download/v0.2.0/DingoTail-0.2.0-macos-x64-corresponding-source.zip) |
| macos-arm64 | [DingoTail-0.2.0-macos-arm64.dmg](macos-arm64/DingoTail-0.2.0-macos-arm64.dmg) | [SHA-256](macos-arm64/DingoTail-0.2.0-macos-arm64-SHA256SUMS.txt) | [DingoTail-0.2.0-macos-arm64-corresponding-source.zip](https://github.com/jpdevoto/dingo-tail-rust/releases/download/v0.2.0/DingoTail-0.2.0-macos-arm64-corresponding-source.zip) |

The Linux/macOS checksum manifests are identical to the release assets. They also list source and branding ZIPs hosted on the [Releases page](https://github.com/jpdevoto/dingo-tail-rust/releases/tag/v0.2.0); download those alongside the binaries to verify every manifest entry. The large corresponding-source archives are kept on Releases rather than duplicated in Git history. Preserve matching source and license notices when redistributing.
