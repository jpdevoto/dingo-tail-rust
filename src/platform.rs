// Copyright (C) 2026 Juan Pablo Devoto
// SPDX-License-Identifier: GPL-3.0-only

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub fn config_dir() -> anyhow::Result<PathBuf> {
    resolve_config(std::env::consts::OS, |key| std::env::var_os(key))
}

fn resolve_config(os: &str, env: impl Fn(&str) -> Option<OsString>) -> anyhow::Result<PathBuf> {
    let absolute = |key: &str| -> anyhow::Result<PathBuf> {
        let path = PathBuf::from(env(key).ok_or_else(|| {
            anyhow::anyhow!(
                "{key} is unavailable; set DINGOTAIL_CONFIG_DIR to an absolute directory"
            )
        })?);
        anyhow::ensure!(path.is_absolute(), "{key} must be an absolute directory");
        Ok(path)
    };
    if env("DINGOTAIL_CONFIG_DIR").is_some() {
        return absolute("DINGOTAIL_CONFIG_DIR");
    }
    match os {
        "windows" => Ok(absolute("APPDATA")?.join("DingoTail")),
        "macos" => Ok(absolute("HOME")?.join("Library/Application Support/DingoTail")),
        "linux" => {
            // XDG specifies ignoring relative or empty values.
            if let Some(value) = env("XDG_CONFIG_HOME") {
                let path = PathBuf::from(value);
                if path.is_absolute() {
                    return Ok(path.join("dingotail"));
                }
            }
            Ok(absolute("HOME")?.join(".config/dingotail"))
        }
        _ => anyhow::bail!("Unsupported operating system: {os}"),
    }
}

pub fn unicode_path(path: &Path) -> anyhow::Result<String> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        anyhow::anyhow!("This path cannot be represented as Unicode; choose a Unicode filename")
    })
}

pub fn validate_renderer(windows: bool, software: bool, dx12: bool) -> Result<(), String> {
    if !windows && (software || dx12) {
        return Err("--dx12 and --renderer software require Windows. Use --renderer auto or hardware on this platform.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_paths_and_override() {
        let base = std::env::temp_dir();
        let env = |key: &str| match key {
            "APPDATA" | "HOME" => Some(base.clone().into_os_string()),
            _ => None,
        };
        assert_eq!(
            resolve_config("windows", env).unwrap(),
            base.join("DingoTail")
        );
        assert_eq!(
            resolve_config("linux", env).unwrap(),
            base.join(".config/dingotail")
        );
        assert_eq!(
            resolve_config("macos", env).unwrap(),
            base.join("Library/Application Support/DingoTail")
        );
        for os in ["windows", "linux", "macos"] {
            assert_eq!(
                resolve_config(os, |_| Some(base.clone().into_os_string())).unwrap(),
                base
            );
            assert!(resolve_config(os, |_| None).is_err());
            assert!(resolve_config(os, |_| Some("relative".into())).is_err());
        }
        assert_eq!(
            resolve_config("linux", |k| if k == "XDG_CONFIG_HOME" {
                Some(base.clone().into_os_string())
            } else {
                None
            })
            .unwrap(),
            base.join("dingotail")
        );
        assert_eq!(
            resolve_config("linux", |k| match k {
                "XDG_CONFIG_HOME" => Some("relative".into()),
                "HOME" => Some(base.clone().into_os_string()),
                _ => None,
            })
            .unwrap(),
            base.join(".config/dingotail")
        );
    }
    #[test]
    fn renderer_platform_contract() {
        for software in [false, true] {
            for dx12 in [false, true] {
                assert!(validate_renderer(true, software, dx12).is_ok());
                assert_eq!(
                    validate_renderer(false, software, dx12).is_ok(),
                    !software && !dx12
                );
            }
        }
    }
}
