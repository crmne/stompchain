//! Which tones the tone browser already has.
//!
//! One question, asked once at startup, off the UI thread: what is published?
//! The answer is a set of file hashes, and a tone in the library is on the site
//! when its portable copy hashes to one of them.
//!
//! Hashes rather than names, because a name is not an identity. Two people can
//! publish "Master of Puppets - Rhythm" and mean different rigs, and one person
//! can rename their own tone without it becoming a different one. The site
//! records the SHA-256 of the file it was given, which is the same question the
//! library already answers about itself.
//!
//! Like the update check, failure is silence. No network, site down, nothing
//! published yet: the column simply says nothing rather than claiming a tone is
//! missing from a place it could not reach.

use std::collections::BTreeSet;
use std::sync::mpsc::{channel, Receiver};

use crate::update::VERSION;

/// Where the tone browser lives. The docs are on `docs.` beside it.
pub const SITE: &str = "https://tonepush.rocks";

/// The read half of the site's API. It needs no credential: reading what has
/// been published is public, and only publishing needs an account.
const TONES: &str = "https://tonepush.rocks/api/v1/tones";

/// How many pages of tones to walk before stopping.
///
/// The index answers fifty at a time. A cap rather than a `while` loop because
/// this runs at startup against a server that may be having a bad day, and an
/// editor that will not open until a paginated API says "no more" is an editor
/// held hostage by somebody else's uptime.
const PAGES: u32 = 20;

/// How many tones to ask about in detail.
///
/// The index does not carry the hashes - they are on the implementations, which
/// only the detail view returns - so learning what is published costs one
/// request per tone. That is fine for a site with a few dozen and plainly wrong
/// for one with thousands, and the fix is not here: the index should return
/// `file_sha256`, or the site should answer "do you have this hash". Until it
/// does, this stops rather than firing a thousand requests at somebody's
/// server every time an editor opens, and says so when it stops.
const DETAILS: usize = 200;

/// Ask the site what it has, in the background.
///
/// The receiver yields at most one value: every published file's hash. Any
/// failure at all yields nothing and hangs up, which the caller reads as "the
/// site has not answered" rather than "the site has nothing".
pub fn published() -> Receiver<BTreeSet<String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        if let Some(hashes) = fetch() {
            let _ = tx.send(hashes);
        }
    });
    rx
}

/// Walk the index and collect every implementation's file hash.
fn fetch() -> Option<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    let mut asked = 0usize;
    for page in 1..=PAGES {
        let body = ureq::get(format!("{TONES}?page={page}"))
            .header("User-Agent", format!("TonePush/{VERSION}"))
            .header("Accept", "application/json")
            .call()
            .ok()?
            .body_mut()
            .read_to_string()
            .ok()?;
        let json: serde_json::Value = serde_json::from_str(&body).ok()?;
        let tones = json.get("tones")?.as_array()?;
        // The last page is the one that comes back empty. Stopping on a short
        // page instead would stop early the day the page size changes.
        if tones.is_empty() {
            break;
        }
        // The summary the index returns carries no hashes - those are on the
        // implementations, which only the detail view has. One request per
        // tone is the cost of an honest answer, and it happens once.
        for tone in tones {
            if asked >= DETAILS {
                eprintln!(
                    "the tone browser has more than {DETAILS} tones; only the first \
                     {DETAILS} were checked. The column may show a published tone as \
                     absent until the site's index carries file_sha256."
                );
                return Some(found);
            }
            let Some(id) = tone.get("id").and_then(|id| id.as_i64()) else {
                continue;
            };
            asked += 1;
            found.extend(hashes_of(id));
        }
    }
    Some(found)
}

/// Every file hash published for one tone, across its implementations.
///
/// A tone can be built more than once - the same song for different pedals -
/// and each of those is a separate file with its own hash. All of them count:
/// the question is whether *this* file is up there, not whether its name is.
fn hashes_of(id: i64) -> Vec<String> {
    let Ok(mut response) = ureq::get(format!("{TONES}/{id}"))
        .header("User-Agent", format!("TonePush/{VERSION}"))
        .header("Accept", "application/json")
        .call()
    else {
        return Vec::new();
    };
    let Ok(body) = response.body_mut().read_to_string() else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Vec::new();
    };
    file_hashes(&json)
}

/// The `file_sha256` of every implementation in a tone's details.
///
/// Separated from the request so the shape of the site's answer can be pinned
/// by a test without a network.
fn file_hashes(details: &serde_json::Value) -> Vec<String> {
    details
        .get("implementations")
        .and_then(|i| i.as_array())
        .map(|implementations| {
            implementations
                .iter()
                .filter_map(|i| i.get("file_sha256")?.as_str())
                // An implementation whose artifact was never uploaded - an
                // external one pointing at CustomTone, say - has no hash, and
                // null is not a hash of anything.
                .filter(|hash| hash.len() == 64)
                .map(|hash| hash.to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

/// Where a person goes to see a tone that is already up there.
pub fn tone_url(hash: &str) -> String {
    format!("{SITE}/tones?q={hash}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape the site actually returns, trimmed to what is read here.
    fn details() -> serde_json::Value {
        serde_json::json!({
            "id": 1,
            "name": "Master of Puppets - Rhythm",
            "implementations": [
                { "id": 1, "file_sha256": "a".repeat(64) },
                { "id": 2, "file_sha256": "B".repeat(64) },
            ]
        })
    }

    #[test]
    fn every_implementations_hash_counts() {
        assert_eq!(
            file_hashes(&details()),
            vec!["a".repeat(64), "b".repeat(64)],
            "the same tone built for two pedals is two files, and either being \
             present means that file is published"
        );
    }

    /// The site stores them lowercase, but a hash that came back shouting is
    /// still the same hash, and comparing it as-is would say "not published".
    #[test]
    fn a_hash_is_matched_regardless_of_case() {
        assert!(file_hashes(&details()).contains(&"b".repeat(64)));
    }

    #[test]
    fn an_implementation_with_no_file_contributes_nothing() {
        let json = serde_json::json!({
            "implementations": [
                { "id": 1, "file_sha256": serde_json::Value::Null },
                { "id": 2 },
                { "id": 3, "file_sha256": "" },
                { "id": 4, "file_sha256": "not-a-hash" },
            ]
        });
        assert!(file_hashes(&json).is_empty());
    }

    #[test]
    fn a_tone_with_no_implementations_is_not_an_error() {
        assert!(file_hashes(&serde_json::json!({ "id": 1 })).is_empty());
        assert!(file_hashes(&serde_json::json!({ "implementations": [] })).is_empty());
    }
}
