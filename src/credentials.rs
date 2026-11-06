// Copyright (C) 2026 Juan Pablo Devoto
// SPDX-License-Identifier: GPL-3.0-only

use crate::model::{LEGACY_CREDENTIAL_SERVICE, Profile};

fn entry(profile: &Profile) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(
        LEGACY_CREDENTIAL_SERVICE,
        &format!("{}@{}:{}", profile.user, profile.host, profile.port),
    )
}
pub fn read(profile: &Profile) -> keyring::Result<Option<String>> {
    missing_is_none(entry(profile)?.get_password())
}
fn missing_is_none(result: keyring::Result<String>) -> keyring::Result<Option<String>> {
    match result {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error),
    }
}
pub fn write(profile: &Profile, secret: &str) -> keyring::Result<()> {
    entry(profile)?.set_password(secret)
}
pub fn delete(profile: &Profile) -> keyring::Result<()> {
    match entry(profile)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_is_distinct_from_store_failure() {
        assert_eq!(
            missing_is_none(Ok("secret".into())).unwrap().as_deref(),
            Some("secret")
        );
        assert!(
            missing_is_none(Err(keyring::Error::NoEntry))
                .unwrap()
                .is_none()
        );
        assert!(
            missing_is_none(Err(keyring::Error::Invalid(
                "store".into(),
                "locked".into()
            )))
            .is_err()
        );
    }
}
