// Copyright (C) 2026 Juan Pablo Devoto
// SPDX-License-Identifier: GPL-3.0-only

use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=assets/brand/dingotail.ico");
    println!("cargo:rerun-if-changed=assets/windows.rc");
    if env::var("CARGO_CFG_TARGET_OS").unwrap() != "windows" {
        return;
    }
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("dingotail.res");
    let sdk = PathBuf::from(env::var("ProgramFiles(x86)").unwrap()).join("Windows Kits/10/bin");
    let mut tools: Vec<_> = fs::read_dir(sdk)
        .expect("Windows SDK required")
        .flatten()
        .map(|e| e.path().join("x64/rc.exe"))
        .filter(|p| p.exists())
        .collect();
    tools.sort();
    let rc = tools
        .last()
        .expect("Windows SDK resource compiler rc.exe required");
    let status = Command::new(rc)
        .current_dir(&root)
        .arg("/nologo")
        .arg("/fo")
        .arg(&out)
        .arg("assets/windows.rc")
        .status()
        .expect("run rc.exe");
    assert!(status.success(), "Resource compilation failed");
    println!("cargo:rustc-link-arg={}", out.display());
}
