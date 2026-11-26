use anyhow::{Context, Result, bail, ensure};
use gtk::glib;
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::net::Ipv6Addr;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::vault::Vault;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub username: String,
    pub port: Option<u16>,
    pub group: String,
    pub key: String,
    pub secret: Option<String>,
}

#[derive(Default, Clone, PartialEq, Eq)]
pub struct Fields {
    pub hostname: String,
    pub username: String,
    pub port: String,
    pub name: String,
    pub group: String,
    pub key: String,
}

#[derive(Debug)]
pub struct InputError(pub &'static str, pub &'static str);

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.1)
    }
}
impl std::error::Error for InputError {}

impl Host {
    pub fn title(&self) -> &str {
        if self.name.is_empty() {
            &self.hostname
        } else {
            &self.name
        }
    }

    pub fn destination(&self) -> String {
        let mut dest = String::new();
        if !self.username.is_empty() {
            dest.push_str(&self.username);
            dest.push('@');
        }
        if self.hostname.contains(':') {
            dest.push_str(&format!("[{}]", self.hostname));
        } else {
            dest.push_str(&self.hostname);
        }
        if let Some(port) = self.port {
            dest.push_str(&format!(":{port}"));
        }
        dest
    }

    pub fn fields(&self) -> Fields {
        Fields {
            hostname: self.hostname.clone(),
            username: self.username.clone(),
            port: self.port.map(|p| p.to_string()).unwrap_or_default(),
            name: self.name.clone(),
            group: self.group.clone(),
            key: self.key.clone(),
        }
    }

    pub fn from_fields(
        fields: Fields,
        original: Option<&Host>,
        password_mode: bool,
    ) -> Result<Self> {
        let hostname = normalize_hostname(fields.hostname.trim())?;
        let username = fields.username.trim();
        let valid_user = username.strip_suffix('$').unwrap_or(username);
        if !username.is_empty()
            && (!valid_user.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                || !valid_user
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_.@\\-".contains(c)))
        {
            return Err(InputError(
                "username",
                "Enter an SSH login name without spaces or shell characters.",
            )
            .into());
        }
        let port = fields.port.trim();
        let port = if port.is_empty() {
            None
        } else {
            Some(
                port.parse::<u16>()
                    .ok()
                    .filter(|n| {
                        *n != 0 && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit())
                    })
                    .ok_or(InputError("port", "Enter a port between 1 and 65535."))?,
            )
        };
        let key = if password_mode || fields.key.trim().is_empty() {
            String::new()
        } else {
            let err = || InputError("key", "Choose a readable private key file.");
            let input = fields.key.trim();
            let path = if let Some(rest) = input.strip_prefix('~') {
                let (user, rel) = rest.split_once('/').unwrap_or((rest, ""));
                let home = if user.is_empty() {
                    glib::home_dir()
                } else {
                    let record = std::process::Command::new("getent")
                        .args(["--", "passwd", user])
                        .output()
                        .map_err(|_| err())?;
                    let home = std::str::from_utf8(&record.stdout)
                        .ok()
                        .and_then(|s| s.split(':').nth(5))
                        .filter(|s| record.status.success() && s.starts_with('/'))
                        .ok_or_else(err)?;
                    PathBuf::from(home)
                };
                home.join(rel)
            } else {
                PathBuf::from(input)
            };
            if !path.is_absolute() || !path.is_file() || File::open(&path).is_err() {
                return Err(err().into());
            }
            path.to_str().ok_or_else(err)?.to_owned()
        };
        Ok(Self {
            id: original.map(|h| h.id.clone()).unwrap_or_else(new_id),
            hostname,
            port,
            username: username.to_owned(),
            name: fields.name.trim().to_owned(),
            group: fields.group.trim().to_owned(),
            key,
            secret: original.and_then(|h| h.secret.clone()),
        })
    }
}

fn new_id() -> String {
    glib::uuid_string_random().replace('-', "")
}

fn normalize_hostname(value: &str) -> Result<String> {
    if value.contains(':') {
        let value = value
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or(value);
        let (ip, scope) = value
            .split_once('%')
            .map_or((value, None), |(ip, scope)| (ip, Some(scope)));
        let err = || InputError("hostname", "Enter a valid IPv6 address.");
        let ip: Ipv6Addr = ip.parse().map_err(|_| err())?;
        return match scope {
            None => Ok(ip.to_string()),
            Some(s)
                if !s.is_empty()
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)) =>
            {
                Ok(format!("{ip}%{s}"))
            }
            _ => Err(err().into()),
        };
    }
    let err = || InputError("hostname", "Enter a hostname, SSH alias, or IP address.");
    let ascii = if value.is_ascii() {
        value.to_string()
    } else {
        glib::hostname_to_ascii(value).ok_or_else(err)?.to_string()
    };
    let valid = ascii.len() <= 254
        && ascii
            .strip_suffix('.')
            .unwrap_or(&ascii)
            .split('.')
            .all(|s| {
                !s.is_empty()
                    && s.len() <= 63
                    && !s.starts_with('-')
                    && !s.ends_with('-')
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            });
    if !valid {
        return Err(err().into());
    }
    Ok(ascii)
}

#[derive(Clone)]
pub struct Store {
    directory: PathBuf,
    vault: Arc<dyn Vault>,
}

const COLUMNS: &str = "id, name, hostname, username, port, group_name, identity_file, secret_id";

fn read_host(row: &Row<'_>) -> rusqlite::Result<Host> {
    Ok(Host {
        id: row.get(0)?,
        name: row.get(1)?,
        hostname: row.get(2)?,
        username: row.get(3)?,
        port: row.get(4)?,
        group: row.get(5)?,
        key: row.get(6)?,
        secret: row.get(7)?,
    })
}

impl Store {
    pub fn desktop() -> Result<Self> {
        Self::open(
            &glib::user_data_dir().join("ssh-manager"),
            Arc::new(crate::vault::DesktopVault),
        )
    }

    pub fn open(directory: &Path, vault: Arc<dyn Vault>) -> Result<Self> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)?;
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(directory.join("hosts.sqlite3"))?;
        let store = Self {
            directory: directory.into(),
            vault,
        };
        store.db()?.execute_batch("CREATE TABLE IF NOT EXISTS hosts (
            id TEXT PRIMARY KEY, name TEXT NOT NULL, hostname TEXT NOT NULL, username TEXT NOT NULL,
            port INTEGER, group_name TEXT NOT NULL, identity_file TEXT NOT NULL, secret_id TEXT UNIQUE);
            CREATE TABLE IF NOT EXISTS unused_secrets (id TEXT PRIMARY KEY);")?;
        Ok(store)
    }

    fn db(&self) -> Result<Connection> {
        let db = Connection::open(self.directory.join("hosts.sqlite3"))?;
        db.busy_timeout(Duration::from_secs(5))?;
        Ok(db)
    }

    fn lock(&self) -> Result<File> {
        // The Python release also takes a flock on the directory across both stores.
        let file = File::open(&self.directory)?;
        file.lock()?;
        Ok(file)
    }

    pub fn hosts(&self) -> Result<Vec<Host>> {
        let db = self.db()?;
        let mut query = db.prepare(&format!("SELECT {COLUMNS} FROM hosts"))?;
        let mut hosts = query
            .query_map([], read_host)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        hosts.sort_by_cached_key(|h| {
            (
                glib::casefold(&h.group),
                glib::casefold(h.title()),
                h.id.clone(),
            )
        });
        Ok(hosts)
    }

    fn lookup(db: &Connection, id: &str) -> Result<Option<Host>> {
        Ok(db
            .query_row(
                &format!("SELECT {COLUMNS} FROM hosts WHERE id=?1"),
                [id],
                read_host,
            )
            .optional()?)
    }

    pub fn get(&self, id: &str) -> Result<Host> {
        Self::lookup(&self.db()?, id)?.context("This host is no longer in the library.")
    }

    pub fn save(
        &self,
        mut host: Host,
        original: Option<&Host>,
        use_vault: bool,
        password: &str,
    ) -> Result<(Host, bool)> {
        let _guard = self.lock()?;
        let mut db = self.db()?;
        ensure!(
            Self::lookup(&db, &host.id)?.as_ref() == original,
            "This host changed in another window. Reopen it and try again."
        );
        let old = original.and_then(|h| h.secret.clone());
        host.secret = if !use_vault {
            None
        } else if password.is_empty() {
            if !original.is_some_and(|h| {
                h.secret.is_some()
                    && h.hostname == host.hostname
                    && h.username == host.username
                    && h.port == host.port
            }) {
                return Err(
                    InputError("password", "Enter the password for this SSH destination.").into(),
                );
            }
            old.clone()
        } else {
            if password.contains(['\r', '\n', '\0']) {
                return Err(InputError("password", "SSH passwords must fit on one line.").into());
            }
            let id = new_id();
            // Commit the cleanup intent first so a crash cannot orphan the new password.
            db.execute("INSERT INTO unused_secrets VALUES (?1)", [&id])?;
            self.vault.put(&id, host.title(), password)?;
            Some(id)
        };
        if use_vault {
            host.key.clear();
        }
        let tx = db.transaction()?;
        tx.execute(
            &format!("INSERT OR REPLACE INTO hosts ({COLUMNS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)"),
            params![
                host.id,
                host.name,
                host.hostname,
                host.username,
                host.port,
                host.group,
                host.key,
                host.secret
            ],
        )?;
        if let Some(id) = &host.secret {
            tx.execute("DELETE FROM unused_secrets WHERE id=?1", [id])?;
        }
        if let Some(id) = old.as_ref().filter(|_| old != host.secret) {
            tx.execute("INSERT INTO unused_secrets VALUES (?1)", [id])?;
        }
        tx.commit()?;
        Ok((host, self.cleanup(&db)?))
    }

    pub fn remove(&self, host: &Host) -> Result<bool> {
        let _guard = self.lock()?;
        let mut db = self.db()?;
        if Self::lookup(&db, &host.id)?.as_ref() != Some(host) {
            bail!("This host changed. Reopen it and try again.");
        }
        let tx = db.transaction()?;
        tx.execute("DELETE FROM hosts WHERE id=?1", [&host.id])?;
        if let Some(id) = &host.secret {
            tx.execute("INSERT INTO unused_secrets VALUES (?1)", [id])?;
        }
        tx.commit()?;
        self.cleanup(&db)
    }

    fn cleanup(&self, db: &Connection) -> Result<bool> {
        let ids = db
            .prepare("SELECT id FROM unused_secrets")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            if self.vault.delete(&id).is_err() {
                return Ok(false);
            }
            db.execute("DELETE FROM unused_secrets WHERE id=?1", [id])?;
        }
        Ok(true)
    }

    pub fn retry_cleanup(&self) -> Result<bool> {
        let _guard = self.lock()?;
        self.cleanup(&self.db()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, os::unix::fs::PermissionsExt, sync::Mutex};

    #[derive(Default)]
    struct MemoryVault {
        passwords: Mutex<HashMap<String, String>>,
        locked: std::sync::atomic::AtomicBool,
    }
    impl Vault for MemoryVault {
        fn put(&self, id: &str, _title: &str, password: &str) -> Result<()> {
            ensure!(
                !self.locked.load(std::sync::atomic::Ordering::SeqCst),
                "keyring locked"
            );
            self.passwords
                .lock()
                .unwrap()
                .insert(id.into(), password.into());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<()> {
            ensure!(
                !self.locked.load(std::sync::atomic::Ordering::SeqCst),
                "keyring locked"
            );
            self.passwords.lock().unwrap().remove(id);
            Ok(())
        }
    }
    struct Fixture {
        store: Store,
        vault: Arc<MemoryVault>,
    }
    impl Fixture {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("ssh-manager-test-{}", new_id()));
            let vault = Arc::new(MemoryVault::default());
            Self {
                store: Store::open(&dir, vault.clone()).unwrap(),
                vault,
            }
        }
        fn password_host(&self) -> Host {
            let h = Host::from_fields(
                Fields {
                    hostname: "db.example".into(),
                    username: "ops".into(),
                    ..Default::default()
                },
                None,
                true,
            )
            .unwrap();
            self.store
                .save(h, None, true, "example-only password")
                .unwrap()
                .0
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.store.directory);
        }
    }

    #[test]
    fn reads_v1_metadata_without_rewriting_existing_credential_references() {
        let f = Fixture::new();
        let db = f.store.db().unwrap();
        db.execute(
            "INSERT INTO hosts (id,name,hostname,username,port,group_name,identity_file,secret_id)
                    VALUES ('v1','Development','::1','ops',2202,'Lab','','old-reference')",
            [],
        )
        .unwrap();
        let host = f.store.get("v1").unwrap();
        assert_eq!(host.destination(), "ops@[::1]:2202");
        let mut renamed = host.clone();
        renamed.name = "New name".into();
        let (saved, clean) = f.store.save(renamed, Some(&host), true, "").unwrap();
        assert!(clean);
        assert_eq!(saved.secret.as_deref(), Some("old-reference"));
        assert_eq!(f.store.hosts().unwrap(), [saved]);
    }
    #[test]
    fn password_is_private_and_destination_bound() {
        let f = Fixture::new();
        let h = f.password_host();
        let db = f.store.directory.join("hosts.sqlite3");
        assert_eq!(db.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(
            f.store.directory.metadata().unwrap().permissions().mode() & 0o777,
            0o700
        );
        let pw = b"example-only password";
        assert!(!fs::read(db).unwrap().windows(pw.len()).any(|s| s == pw));
        let mut other = h.clone();
        other.port = Some(2222);
        assert!(f.store.save(other, Some(&h), true, "").is_err());
        assert_eq!(f.store.get(&h.id).unwrap(), h);
    }
    #[test]
    fn rotates_and_retires_passwords_and_rejects_stale_edits() {
        let f = Fixture::new();
        let h = f.password_host();
        let (replacement, _) = f
            .store
            .save(h.clone(), Some(&h), true, "replacement")
            .unwrap();
        assert_ne!(replacement.secret, h.secret);
        assert!(f.store.save(h.clone(), Some(&h), true, "stale").is_err());
        assert_eq!(f.vault.passwords.lock().unwrap().len(), 1);
        let (key_host, _) = f
            .store
            .save(replacement.clone(), Some(&replacement), false, "")
            .unwrap();
        assert!(key_host.secret.is_none());
        assert!(f.vault.passwords.lock().unwrap().is_empty());
    }
    #[test]
    fn interrupted_db_write_keeps_the_prior_password() {
        let f = Fixture::new();
        let h = f.password_host();
        f.store.db().unwrap().execute_batch("CREATE TRIGGER fail BEFORE INSERT ON hosts BEGIN SELECT RAISE(ABORT,'aborted'); END;").unwrap();
        assert!(f.store.save(h.clone(), Some(&h), true, "unused").is_err());
        assert_eq!(f.store.get(&h.id).unwrap(), h);
        assert!(f.store.retry_cleanup().unwrap());
        assert_eq!(f.vault.passwords.lock().unwrap().len(), 1);
        assert!(
            f.vault
                .passwords
                .lock()
                .unwrap()
                .contains_key(h.secret.as_ref().unwrap())
        );
    }
    #[test]
    fn locked_keyring_does_not_overwrite_and_removal_is_retried() {
        use std::sync::atomic::Ordering;
        let f = Fixture::new();
        let h = f.password_host();
        f.vault.locked.store(true, Ordering::SeqCst);
        assert!(
            f.store
                .save(h.clone(), Some(&h), true, "replacement")
                .is_err()
        );
        assert_eq!(f.store.get(&h.id).unwrap(), h);
        assert!(!f.store.remove(&h).unwrap());
        assert!(f.store.hosts().unwrap().is_empty());
        f.vault.locked.store(false, Ordering::SeqCst);
        let next = Store::open(&f.store.directory, f.vault.clone()).unwrap();
        assert!(next.retry_cleanup().unwrap());
        assert!(f.vault.passwords.lock().unwrap().is_empty());
    }
    #[test]
    fn validates_hosts_and_preserves_openssh_arguments() {
        for value in [
            "host-alias",
            "127.0.0.1",
            "[2001:db8::1]",
            "fe80::8%eth0",
            "db.example.",
            "münchen.example",
        ] {
            let h = Host::from_fields(
                Fields {
                    hostname: value.into(),
                    ..Default::default()
                },
                None,
                false,
            )
            .unwrap();
            let args = crate::ssh::arguments(&h);
            assert_eq!(args[args.len() - 2..], ["--", &h.hostname]);
            assert!(!args.iter().any(|a| a == "-p" || a == "-l"));
        }
        for value in [
            "",
            "-oProxyCommand=test",
            "user@host",
            "fe80::8%bad scope",
            "host | cmd",
        ] {
            assert!(normalize_hostname(value).is_err());
        }
        let f = Fixture::new();
        let mut h = f.password_host();
        h.key = "/path/key with spaces".into();
        h.secret = None;
        assert!(
            crate::ssh::arguments(&h)
                .windows(2)
                .any(|pair| pair == ["-i", "/path/key with spaces"])
        );
    }
}
