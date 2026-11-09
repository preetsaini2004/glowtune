use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use crate::{Error, Result, Theme, Warning};

/// Returns the embedded theme ids and original TOML text in display order.
pub fn bundled() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "midnight-glass",
            include_str!("../../../themes/midnight-glass.toml"),
        ),
        ("aurora", include_str!("../../../themes/aurora.toml")),
        ("ember", include_str!("../../../themes/ember.toml")),
    ]
}

/// Metadata for one available theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeInfo {
    /// Safe file stem used to load the theme.
    pub id: String,
    /// Human-readable theme name.
    pub name: String,
    /// Theme creator.
    pub author: String,
    /// Whether the entry comes from the embedded themes.
    pub bundled: bool,
}

/// Bundled themes plus overrides and custom themes in a user directory.
#[derive(Clone, Debug)]
pub struct ThemeStore {
    user_dir: PathBuf,
}

pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn slug(name: &str) -> String {
    let mut id = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            id.push(character.to_ascii_lowercase());
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
        if id.len() == 48 {
            break;
        }
    }
    let id = id.trim_end_matches('-');
    if id.is_empty() {
        "theme".into()
    } else {
        id.into()
    }
}

/// Stages data beside its destination and publishes it atomically.
/// Hard linking for imports prevents overwriting a concurrent import.
fn atomic_write(path: &Path, text: &str, overwrite: bool) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let (temp, mut file) = loop {
        let temp = parent.join(format!(
            ".glowtune-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        if overwrite {
            fs::rename(&temp, path)
        } else {
            fs::hard_link(&temp, path)
        }
    })();
    if let Err(error) = fs::remove_file(&temp)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(path = %temp.display(), %error, "could not remove theme staging file");
    }
    result
}

impl ThemeStore {
    /// Opens a user theme directory, creating it and its parents if needed.
    pub fn new(user_dir: impl Into<PathBuf>) -> Result<Self> {
        let user_dir = user_dir.into();
        fs::create_dir_all(&user_dir)?;
        Ok(Self { user_dir })
    }

    fn path(&self, id: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err(Error::InvalidId(id.into()));
        }
        Ok(self.user_dir.join(format!("{id}.toml")))
    }

    /// Lists bundled entries first, followed by user entries sorted by name and id.
    /// Unreadable or syntactically invalid user files are logged and skipped.
    pub fn list(&self) -> Vec<ThemeInfo> {
        let mut users = Vec::new();
        match fs::read_dir(&self.user_dir) {
            Ok(entries) => {
                for entry in entries {
                    let entry = match entry {
                        Ok(entry) => entry,
                        Err(error) => {
                            tracing::warn!(%error, "could not read theme directory entry");
                            continue;
                        }
                    };
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                        continue;
                    }
                    let Some(id) = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .filter(|id| valid_id(id))
                    else {
                        continue;
                    };
                    match fs::read_to_string(&path)
                        .map_err(Error::from)
                        .and_then(|text| Theme::from_toml_str(&text))
                    {
                        Ok((theme, _)) => users.push(ThemeInfo {
                            id: id.into(),
                            name: theme.meta.name,
                            author: theme.meta.author,
                            bundled: false,
                        }),
                        Err(error) => {
                            tracing::warn!(path = %path.display(), %error, "skipping unreadable theme")
                        }
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "could not list user themes"),
        }
        users.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        let mut entries = Vec::new();
        for (id, text) in bundled() {
            if !users.iter().any(|user| user.id == id)
                && let Ok((theme, _)) = Theme::from_toml_str(text)
            {
                entries.push(ThemeInfo {
                    id: id.into(),
                    name: theme.meta.name,
                    author: theme.meta.author,
                    bundled: true,
                });
            }
        }
        entries.extend(users);
        entries
    }

    /// Loads a user override when present, otherwise the embedded theme.
    pub fn load(&self, id: &str) -> Result<(Theme, Vec<Warning>)> {
        match fs::read_to_string(self.path(id)?) {
            Ok(text) => return Theme::from_toml_str(&text),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        bundled()
            .into_iter()
            .find(|(key, _)| *key == id)
            .ok_or_else(|| Error::NotFound(id.into()))
            .and_then(|(_, text)| Theme::from_toml_str(text))
    }

    /// Atomically saves a sanitized theme under its name's slug, replacing user data at that id.
    pub fn save(&self, theme: &Theme) -> Result<String> {
        let id = slug(&theme.meta.name);
        let (theme, warnings) = theme.clone().sanitized();
        for warning in warnings {
            tracing::warn!(%warning, "sanitized saved theme");
        }
        atomic_write(&self.path(&id)?, &theme.to_toml_string(), true)?;
        Ok(id)
    }

    /// Imports valid TOML, recovering bad fields and adding a numeric suffix on id collision.
    pub fn import(&self, path: impl AsRef<Path>) -> Result<String> {
        let (theme, warnings) = Theme::from_toml_str(&fs::read_to_string(path)?)?;
        for warning in warnings {
            tracing::warn!(%warning, "sanitized imported theme");
        }
        let base = slug(&theme.meta.name);
        let text = theme.to_toml_string();
        let mut number = 1_u64;
        loop {
            let id = if number == 1 {
                base.clone()
            } else {
                let suffix = format!("-{number}");
                format!(
                    "{}{}",
                    base[..base.len().min(48 - suffix.len())].trim_end_matches('-'),
                    suffix
                )
            };
            if !bundled().iter().any(|(key, _)| *key == id) {
                match atomic_write(&self.path(&id)?, &text, false) {
                    Ok(()) => return Ok(id),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
            }
            number = number
                .checked_add(1)
                .ok_or_else(|| io::Error::other("no available theme ids"))?;
        }
    }

    /// Atomically exports a loaded theme to a destination file, replacing it if present.
    pub fn export(&self, id: &str, dest_path: impl AsRef<Path>) -> Result<()> {
        let (theme, _) = self.load(id)?;
        atomic_write(dest_path.as_ref(), &theme.to_toml_string(), true)?;
        Ok(())
    }

    /// Deletes a user theme; removing an override reveals the bundled original.
    pub fn delete(&self, id: &str) -> Result<()> {
        match fs::remove_file(self.path(id)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if bundled().iter().any(|(key, _)| *key == id) {
                    Err(Error::BundledReadOnly(id.into()))
                } else {
                    Err(Error::NotFound(id.into()))
                }
            }
            Err(error) => Err(error.into()),
        }
    }
}
