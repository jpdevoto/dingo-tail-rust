# Platform requirements and status

Windows x64 has an existing release in this export. Linux x64 and macOS Intel/Apple Silicon implementations and packaging configurations exist, but native compilation, installation and desktop validation remain unconfirmed. Windows validation also remains incomplete beyond the existing project evidence and this documentation session.

## Native build requirements

- Windows: Rust with x64 MSVC, Visual Studio C++ build tools, Windows SDK including the x64 resource compiler. OpenGL or compatible DX12 graphics at runtime.
- Linux: Rust, C/C++ compiler, pkg-config and D-Bus development headers. X11/Wayland, OpenGL/EGL and an XDG desktop portal backend at runtime. Saving credentials needs session D-Bus and an unlocked Secret Service provider.
- macOS: Rust and Xcode command-line tools. The packaging baseline is macOS 13; set MACOSX_DEPLOYMENT_TARGET=13.0 when building. OpenGL and Keychain integration need native validation.

Use `cargo build --release --locked`, optionally with `--target` and one of `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, or `aarch64-apple-darwin` after installing the appropriate toolchain target. Native packaging is required; the script does not provide a cross-compilation SDK.

Python 3.11+ is needed for packaging. Debian packaging uses dpkg-deb and dpkg-shlibdeps; RPM uses rpmbuild. macOS packaging uses sips, iconutil, codesign and hdiutil. Linux packages should be built on the oldest intended family baseline. The existing workflow records platform dependency installation commands.

Windows/Linux packages are unsigned. macOS packaging uses ad-hoc signing, not Developer ID signing or notarization. Linux/macOS use OpenGL and reject Windows DX12/software-renderer overrides. There are no Linux or macOS release binaries in this export.

The minimum Rust version is not declared in Cargo.toml. Existing project documentation and workflow pin 1.98.1; no toolchain was installed or checked during export. Registry access or a populated Cargo cache is necessary because dependency sources are not vendored here. The packaging script creates vendored corresponding-source archives for future distributions.
