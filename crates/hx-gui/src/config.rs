//! Local settings that outlive a run: favorites today, rig profiles and
//! preferences next. One small JSON file the user could open and read, loaded
//! leniently so a hand-edit or an older version never loses the app.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A favorited preset location. A preset lives at a (setlist, slot); the star
/// follows the slot, so re-saving a different preset there keeps the star. An
/// HX Stomp has a single setlist, so `setlist` is 0 on that hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Favorite {
    pub setlist: i64,
    pub slot: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub favorites: Vec<Favorite>,
    /// The credential for publishing, got by pairing this computer with an
    /// account. It is a session on the site like any other, so signing out
    /// there ends it here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Who that account is, for the editor to say so without asking the site
    /// every time it starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

impl Config {
    /// Remember the account this computer is signed in as.
    pub fn sign_in(&mut self, token: String, account: String) {
        self.token = Some(token);
        self.account = Some(account);
        self.save();
    }

    /// Forget it. The session is still live on the site until it is revoked
    /// there, which is the honest thing to say rather than pretending this
    /// reaches across the network.
    pub fn sign_out(&mut self) {
        self.token = None;
        self.account = None;
        self.save();
    }
}

/// `~/.config/tonepush/config.json`, mirroring where extracted resources
/// live. `None` only when there is no home to write into.
fn config_path() -> Option<PathBuf> {
    hx_catalog::home::config()
}

impl Config {
    /// Read the file, or start empty. A missing or unreadable file is not worth
    /// bothering anyone with: the app simply has no favorites yet.
    pub fn load() -> Self {
        config_path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Write the file, creating the directory. Failures are silent: losing a
    /// star is not worth interrupting an edit for.
    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn is_favorite(&self, setlist: i64, slot: i64) -> bool {
        self.favorites
            .iter()
            .any(|f| f.setlist == setlist && f.slot == slot)
    }

    /// Toggle a favorite and persist immediately.
    pub fn toggle_favorite(&mut self, setlist: i64, slot: i64) {
        if let Some(pos) = self
            .favorites
            .iter()
            .position(|f| f.setlist == setlist && f.slot == slot)
        {
            self.favorites.remove(pos);
        } else {
            self.favorites.push(Favorite { setlist, slot });
        }
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_adds_then_removes() {
        let mut config = Config::default();
        assert!(!config.is_favorite(0, 5));
        config.favorites.push(Favorite {
            setlist: 0,
            slot: 5,
        });
        assert!(config.is_favorite(0, 5));
        assert!(!config.is_favorite(0, 6));
        assert!(!config.is_favorite(1, 5));
    }

    #[test]
    fn round_trips_through_json() {
        let config = Config {
            favorites: vec![
                Favorite {
                    setlist: 0,
                    slot: 3,
                },
                Favorite {
                    setlist: 2,
                    slot: 11,
                },
            ],
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.favorites, config.favorites);
    }

    /// A file written before there was an account still loads, and one written
    /// while signed out does not carry empty fields for it.
    #[test]
    fn an_account_is_remembered_and_forgotten() {
        let mut config = Config::default();
        assert!(config.token.is_none());

        config.token = Some("secret".to_owned());
        config.account = Some("Carmine".to_owned());
        let json = serde_json::to_string(&config).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.token.as_deref(), Some("secret"));
        assert_eq!(back.account.as_deref(), Some("Carmine"));

        config.token = None;
        config.account = None;
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("token"), "{json}");
    }

    #[test]
    fn an_empty_or_broken_file_is_just_no_favorites() {
        let back: Config = serde_json::from_str("{}").unwrap();
        assert!(back.favorites.is_empty());
    }
}
