# Copyright (C) 2026 Juan Pablo Devoto
# SPDX-License-Identifier: GPL-3.0-only
"""Native release packager. Linux/macOS paths are prepared, not locally validated."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import struct
import sys
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DOCS = ('README.md', 'PORTABILITY.md', 'LICENSE', 'COPYRIGHT', 'BRANDING.md')
TARGETS = {
    'windows-x64': ('x86_64-pc-windows-msvc', 'dingotail.exe'),
    'linux-deb-x64': ('x86_64-unknown-linux-gnu', 'dingotail'),
    'linux-rpm-x64': ('x86_64-unknown-linux-gnu', 'dingotail'),
    'macos-x64': ('x86_64-apple-darwin', 'dingotail'),
    'macos-arm64': ('aarch64-apple-darwin', 'dingotail'),
}

def run(*args, **kwargs):
    subprocess.run([str(a) for a in args], check=True, **kwargs)

def copy(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)

def zip_tree(source, destination):
    with zipfile.ZipFile(destination, 'w', zipfile.ZIP_DEFLATED, compresslevel=6, strict_timestamps=False) as archive:
        hashes = {}
        for path in sorted(source.rglob('*')):
            if path.is_file():
                name = path.relative_to(source).as_posix()
                archive.write(path, name)
                hashes[name] = hashlib.sha256(path.read_bytes()).hexdigest()
        archive.writestr('SOURCE-SHA256.json', json.dumps(hashes, indent=2))
    with zipfile.ZipFile(destination) as archive:
        assert archive.testzip() is None

def source_archive(stage, output):
    # Explicit source allowlist: never include runtime profiles, private keys or QA output.
    source = stage / 'source'
    source.mkdir()
    for path in ROOT.iterdir():
        if path.is_file() and (path.suffix in ('.md', '.toml', '.lock', '.rs', '.ps1') or path.name in ('LICENSE', 'COPYRIGHT', '.gitignore')):
            copy(path, source / path.name)
    for folder in ('src', 'assets', 'licenses', 'tools', 'packaging', 'docs', '.github/workflows'):
        for path in (ROOT / folder).rglob('*'):
            if path.is_file() and '__pycache__' not in path.parts:
                copy(path, source / path.relative_to(ROOT))
    copy(ROOT / 'BRANDING.md', source / 'assets/brand/BRANDING.md')
    print('Vendoring locked sources...', flush=True)
    with (stage / 'vendor.log').open('w', encoding='utf-8') as log:
        run('cargo', 'vendor', '--locked', '--offline', '--respect-source-config', source / 'vendor', cwd=ROOT, stdout=subprocess.DEVNULL, stderr=log)
    (source / '.cargo').mkdir()
    (source / '.cargo/config.toml').write_text('[source.crates-io]\nreplace-with = "vendored-sources"\n[source.vendored-sources]\ndirectory = "vendor"\n', encoding='utf-8')
    run('cargo', 'metadata', '--locked', '--offline', '--format-version', '1', '--manifest-path', source / 'Cargo.toml', cwd=source, stdout=subprocess.DEVNULL)
    print('Collecting dependency notices...', flush=True)
    notices = ['DingoTail dependency notices. All lockfile platforms are included; not all dependencies are linked into each binary. Full upstream sources accompany this release.']
    packages = []
    for crate in sorted((source / 'vendor').iterdir()):
        if not crate.is_dir():
            continue
        manifest = tomllib.loads((crate / 'Cargo.toml').read_text(encoding='utf-8'))['package']
        packages.append({k: manifest.get(k) for k in ('name', 'version', 'license', 'license-file')})
        notices.append('\n' + manifest['name'] + ' ' + manifest['version'] + '\n' + str(manifest.get('license', 'See upstream notices')))
        for path in sorted(crate.rglob('*')):
            if path.is_file() and (path.name.lower().startswith(('license', 'licence', 'copying', 'notice', 'copyright')) or path.name.lower() in ('ofl.txt', 'ufl.txt')):
                notices.append('\n--- ' + path.relative_to(crate).as_posix() + ' ---\n' + path.read_text(encoding='utf-8', errors='replace'))
    (source / 'licenses/DEPENDENCY-NOTICES.txt').write_text('\n'.join(notices), encoding='utf-8')
    (source / 'licenses/DEPENDENCIES.json').write_text(json.dumps(packages, indent=2), encoding='utf-8')
    print('Writing matching source archive...', flush=True)
    zip_tree(source, output)
    return source

def linux_tree(stage, source, binary):
    tree = stage / 'linux'
    copy(binary, tree / 'usr/bin/dingotail')
    (tree / 'usr/bin/dingotail').chmod(0o755)
    copy(ROOT / 'packaging/dingotail.desktop', tree / 'usr/share/applications/dingotail.desktop')
    for size in (16, 24, 32, 48, 64, 128, 256):
        copy(ROOT / f'assets/brand/icon-{size}.png', tree / f'usr/share/icons/hicolor/{size}x{size}/apps/dingotail.png')
    for name in DOCS:
        copy(source / name, tree / 'usr/share/doc/dingotail' / name)
    shutil.copytree(source / 'licenses', tree / 'usr/share/doc/dingotail/licenses')
    shutil.copytree(source / 'docs/screenshots', tree / 'usr/share/doc/dingotail/docs/screenshots')
    return tree

def validate_binary(path, target):
    with path.open('rb') as stream:
        header = stream.read(64)
        if target == 'windows-x64':
            if header[:2] != b'MZ': raise ValueError('Expected Windows PE executable')
            stream.seek(struct.unpack_from('<I', header, 60)[0])
            pe = stream.read(6)
            if pe[:4] != b'PE\0\0' or struct.unpack_from('<H', pe, 4)[0] != 0x8664:
                raise ValueError('Expected Windows x64 executable')
        elif target.startswith('linux'):
            if header[:6] != b'\x7fELF\x02\x01' or struct.unpack_from('<H', header, 18)[0] != 62:
                raise ValueError('Expected Linux x64 ELF executable')
        else:
            cpu = 0x0100000C if target == 'macos-arm64' else 0x01000007
            if header[:4] != b'\xcf\xfa\xed\xfe' or struct.unpack_from('<I', header, 4)[0] != cpu:
                raise ValueError('Expected matching thin macOS Mach-O executable')

def package(args):
    triple, executable = TARGETS[args.target]
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    binary = args.binary.resolve() if args.binary else ROOT / 'target' / triple / 'release' / executable
    if not binary.is_file():
        raise SystemExit(f'Missing built binary: {binary}; pass --binary explicitly for a native build')
    validate_binary(binary, args.target)
    expected_host = 'win32' if args.target.startswith('windows') else 'linux' if args.target.startswith('linux') else 'darwin'
    if sys.platform != expected_host:
        raise SystemExit(f'Package {args.target} on its native OS, not {sys.platform}')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    prefix = f'DingoTail-{version}-{args.target}'
    artifacts = []
    with tempfile.TemporaryDirectory(prefix='dingotail-package-') as directory:
        stage = Path(directory)
        archive = output / f'{prefix}-corresponding-source.zip'
        source = source_archive(stage, archive)
        artifacts.append(archive)
        if args.target == 'windows-x64':
            artifact = output / f'{prefix}.zip'
            with zipfile.ZipFile(artifact, 'w', zipfile.ZIP_DEFLATED, strict_timestamps=False) as z:
                z.write(binary, executable)
                for name in DOCS:
                    z.write(source / name, name)
                for path in (source / 'licenses').rglob('*'):
                    if path.is_file(): z.write(path, path.relative_to(source).as_posix())
                for path in (source / 'docs/screenshots').glob('*'):
                    z.write(path, path.relative_to(source).as_posix())
                z.write(archive, archive.name, compress_type=zipfile.ZIP_STORED)
            with zipfile.ZipFile(artifact) as z:
                assert z.testzip() is None
        elif args.target.startswith('linux'):
            tree = linux_tree(stage, source, binary)
            if args.target == 'linux-deb-x64':
                control = tree / 'DEBIAN'
                control.mkdir()
                # Resolve ELF dependencies on the baseline build host, not by guessing sonames.
                work = stage / 'debdeps'
                (work / 'debian').mkdir(parents=True)
                (work / 'debian/control').write_text('Source: dingotail\nSection: utils\nPriority: optional\nMaintainer: Juan Pablo Devoto <jpdevoto@users.noreply.github.com>\n\nPackage: dingotail\nArchitecture: amd64\nDescription: Local and SSH log viewer\n')
                deps = subprocess.check_output(['dpkg-shlibdeps', '-O', '-e' + str(tree / 'usr/bin/dingotail')], cwd=work, text=True).strip().removeprefix('shlibs:Depends=')
                (control / 'control').write_text(f'Package: dingotail\nVersion: {version}\nSection: utils\nPriority: optional\nArchitecture: amd64\nMaintainer: Juan Pablo Devoto <jpdevoto@users.noreply.github.com>\nDepends: {deps}, libx11-6, libxcursor1, libxi6, libxrandr2, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-cursor0, libegl1, libgl1, xdg-desktop-portal\nRecommends: gnome-keyring, xdg-desktop-portal-gtk | xdg-desktop-portal-kde\nDescription: Native local and SSH log viewer\n Requires an OpenGL desktop and a Secret Service for saved credentials.\n')
                artifact = output / f'{prefix}.deb'
                run('dpkg-deb', '--root-owner-group', '--build', tree, artifact)
            else:
                rpm = stage / 'rpm'
                for name in ('BUILD', 'RPMS', 'SOURCES', 'SPECS', 'SRPMS', 'BUILDROOT'):
                    (rpm / name).mkdir(parents=True)
                spec = (ROOT / 'packaging/dingotail.spec').read_text().replace('@VERSION@', version).replace('@TREE@', str(tree))
                (rpm / 'SPECS/dingotail.spec').write_text(spec)
                run('rpmbuild', '-bb', '--define', f'_topdir {rpm}', rpm / 'SPECS/dingotail.spec')
                built = list((rpm / 'RPMS').rglob('*.rpm'))
                if len(built) != 1: raise RuntimeError(f'Expected one RPM, found {built}')
                artifact = output / f'{prefix}.rpm'
                copy(built[0], artifact)
        else:
            import plistlib
            app = stage / 'dmg/DingoTail.app/Contents'
            copy(binary, app / 'MacOS/dingotail')
            (app / 'MacOS/dingotail').chmod(0o755)
            resources = app / 'Resources'
            resources.mkdir()
            iconset = stage / 'DingoTail.iconset'
            iconset.mkdir()
            for size in (16, 32, 128, 256, 512):
                for scale in (1, 2):
                    name = f'icon_{size}x{size}' + ('@2x' if scale == 2 else '') + '.png'
                    run('sips', '-z', size * scale, size * scale, ROOT / 'assets/brand/mascot.png', '--out', iconset / name, stdout=subprocess.DEVNULL)
            run('iconutil', '-c', 'icns', iconset, '-o', resources / 'DingoTail.icns')
            with (app / 'Info.plist').open('wb') as f:
                plistlib.dump({'CFBundleExecutable': 'dingotail', 'CFBundleIdentifier': 'com.dingodicom.dingotail', 'CFBundleName': 'DingoTail', 'CFBundlePackageType': 'APPL', 'CFBundleShortVersionString': version, 'CFBundleVersion': version, 'CFBundleIconFile': 'DingoTail.icns', 'LSMinimumSystemVersion': '13.0', 'NSHighResolutionCapable': True}, f)
            for name in DOCS: copy(source / name, resources / name)
            shutil.copytree(source / 'licenses', resources / 'licenses')
            shutil.copytree(source / 'docs/screenshots', resources / 'docs/screenshots')
            # Ad-hoc signing permits Apple Silicon execution; this is NOT Developer ID signing/notarization.
            run('codesign', '--force', '--sign', '-', stage / 'dmg/DingoTail.app')
            os.symlink('/Applications', stage / 'dmg/Applications')
            artifact = output / f'{prefix}.dmg'
            run('hdiutil', 'create', '-volname', 'DingoTail', '-srcfolder', stage / 'dmg', '-ov', '-format', 'UDZO', artifact)
        artifacts.append(artifact)
        brand = output / f'{prefix}-brand-assets.zip'
        zip_tree(source / 'assets/brand', brand)
        artifacts.append(brand)
    (output / f'{prefix}-SHA256SUMS.txt').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in artifacts), encoding='utf-8')
    for path in artifacts: print(path, path.stat().st_size, 'bytes')

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', choices=TARGETS, default='windows-x64')
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    package(parser.parse_args())
