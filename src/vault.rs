use anyhow::{Context, Result};
use gtk::{gio, prelude::*};
use libsecret::{Schema, SchemaAttributeType, SchemaFlags, SearchFlags, prelude::ItemExt};
use std::collections::HashMap;
use zeroize::Zeroizing;

pub trait Vault: Send + Sync {
    fn put(&self, id: &str, title: &str, password: &str) -> Result<()>;
    fn delete(&self, id: &str) -> Result<()>;
}

pub struct DesktopVault;

fn schema() -> Schema {
    Schema::new(
        "org.sshmanager.Desktop.Password",
        SchemaFlags::NONE,
        HashMap::from([("credential", SchemaAttributeType::String)]),
    )
}

impl DesktopVault {
    pub fn get(&self, id: &str) -> Result<Zeroizing<String>> {
        let password = libsecret::password_lookup_sync(
            Some(&schema()),
            HashMap::from([("credential", id)]),
            gio::Cancellable::NONE,
        )
        .context("Could not read the desktop keyring.")?
        .context("The saved password is unavailable. Edit the host to save it again.")?;
        Ok(Zeroizing::new(password.to_string()))
    }
}

impl Vault for DesktopVault {
    fn put(&self, id: &str, title: &str, password: &str) -> Result<()> {
        libsecret::password_store_sync(
            Some(&schema()),
            HashMap::from([("credential", id)]),
            Some(&libsecret::COLLECTION_DEFAULT),
            &format!("SSH Manager · {title}"),
            password,
            gio::Cancellable::NONE,
        )
        .context("Could not save the password in the desktop keyring.")
    }

    fn delete(&self, id: &str) -> Result<()> {
        // The convenience clear function skips locked items. Delete each match
        // explicitly so an unsuccessful unlock remains in the cleanup journal.
        let items = libsecret::password_search_sync(
            Some(&schema()),
            HashMap::from([("credential", id)]),
            SearchFlags::ALL,
            gio::Cancellable::NONE,
        )?;
        for item in items {
            item.downcast::<libsecret::Item>()
                .ok()
                .context("The keyring could not remove this item.")?
                .delete_sync(gio::Cancellable::NONE)?;
        }
        Ok(())
    }
}
