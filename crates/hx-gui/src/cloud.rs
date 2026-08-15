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
//! Like the update check, failure is silence. With no network or a site that is
//! down, the column says nothing rather than claiming a tone is missing from a
//! place it could not reach. A successful empty answer is different: it draws
//! the outline clouds that publish the first tones.

use std::collections::BTreeSet;
use std::sync::mpsc::{channel, Receiver};

use crate::update::VERSION;

/// Where the tone browser lives. The docs are on `docs.` beside it.
///
/// `TONEPUSH_SITE` points it somewhere else, which is how the publishing side
/// is exercised against a site running on this machine. Nothing in the app
/// offers to change it: a person has one tone browser.
pub fn site() -> String {
    std::env::var("TONEPUSH_SITE").unwrap_or_else(|_| SITE.to_owned())
}

const SITE: &str = "https://tonepush.rocks";

/// The read half of the site's API. It needs no credential: reading what has
/// been published is public, and only publishing needs an account.
fn tones() -> String {
    format!("{}/api/v1/tones", site())
}

/// How many pages of tones to walk before stopping.
///
/// The index answers fifty at a time. A cap rather than a `while` loop because
/// this runs against a server that may be having a bad day. The editor opens
/// while it works, but the task still needs a ceiling rather than asking an
/// unbounded number of pages when the server's pagination is broken.
const PAGES: u32 = 100;

/// How many tones to ask about one at a time, when the index will not say.
///
/// The index carries `file_sha256s` now, so the ordinary path is one request
/// per page and this is never reached. It is the fallback for a site that has
/// not deployed that yet, where the hashes live only on a tone's details and
/// learning them costs a request each. Capped so an editor opening against an
/// older site with a large library does not fire a thousand requests at it,
/// and it says so when it stops rather than quietly reporting less.
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
    // Reuse one connection while walking the index. The tone browser is well
    // beyond its first thousand listings now; a fresh TLS handshake for every
    // page made the truthful answer arrive several seconds later than needed.
    let agent = ureq::Agent::new_with_defaults();
    for page in 1..=PAGES {
        let body = agent
            .get(format!("{}?page={page}", tones()))
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
        for tone in tones {
            // The index carries the hashes itself, which makes the whole
            // question one request per page.
            if let Some(listed) = summary_hashes(tone) {
                found.extend(listed);
                continue;
            }
            // An older site only has them on a tone's details, so this asks -
            // once per tone, which is why it is capped. Kept so the editor
            // still works against a site that has not deployed the index
            // change yet, and it costs nothing once that lands.
            if asked >= DETAILS {
                eprintln!(
                    "the tone browser has more than {DETAILS} tones and its index does \
                     not carry file_sha256, so only the first {DETAILS} were checked."
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

/// The hashes a tone's index entry lists, when the site puts them there.
///
/// `None` means the field is absent, which is a site old enough to keep them
/// only on the details - a different thing from a tone that has none, which is
/// an empty list and needs no second request.
fn summary_hashes(tone: &serde_json::Value) -> Option<Vec<String>> {
    let listed = tone.get("file_sha256s")?.as_array()?;
    Some(
        listed
            .iter()
            .filter_map(|hash| hash.as_str())
            .filter(|hash| hash.len() == 64)
            .map(|hash| hash.to_ascii_lowercase())
            .collect(),
    )
}

/// Every file hash published for one tone, across its implementations.
///
/// A tone can be built more than once - the same song for different pedals -
/// and each of those is a separate file with its own hash. All of them count:
/// the question is whether *this* file is up there, not whether its name is.
fn hashes_of(id: i64) -> Vec<String> {
    let Ok(mut response) = ureq::get(format!("{}/{id}", tones()))
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
    format!("{}/tones?q={hash}", site())
}

/// Signing this computer in, and publishing once it is.
///
/// The site signs people in by mailing them a link, and that mail is as likely
/// to be read on a phone as on the machine running this. So the editor cannot
/// wait for a browser to come back to it the way a command-line tool does: it
/// asks the site for a pairing, shows the code, and asks about that pairing
/// until somebody approves it from wherever they happen to be.
fn pairings() -> String {
    format!("{}/api/v1/pairings", site())
}

/// A pairing in progress: what to show the person, and what to ask about.
#[derive(Debug, Clone)]
pub struct Pairing {
    pub code: String,
    /// The page that approves it. Opened for them, and worth showing in case
    /// the browser that opened is not the one they are signed in to.
    pub url: String,
}

/// How the pairing ended.
pub enum Linked {
    /// Signed in, with the credential and who it belongs to.
    In { token: String, account: String },
    /// Nobody approved it in time, or the site stopped answering.
    GaveUp(String),
}

/// Ask the site to start a pairing.
pub fn start_pairing() -> Result<Pairing, String> {
    let mut response = ureq::post(pairings())
        .header("User-Agent", agent())
        .header("Accept", "application/json")
        .send_empty()
        .map_err(|e| format!("the tone browser did not answer: {e}"))?;
    let body = read_json(&mut response)?;
    let (Some(code), Some(url)) = (
        body.get("code").and_then(|c| c.as_str()),
        body.get("url").and_then(|u| u.as_str()),
    ) else {
        return Err("the tone browser did not offer a code".to_owned());
    };
    Ok(Pairing {
        code: code.to_owned(),
        url: url.to_owned(),
    })
}

/// Ask about a pairing once. `Ok(None)` means nobody has approved it yet.
pub fn poll_pairing(code: &str) -> Result<Option<Linked>, String> {
    let mut response = ureq::get(format!("{}/{code}", pairings()))
        .header("User-Agent", agent())
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("the tone browser stopped answering: {e}"))?;
    let body = read_json(&mut response)?;
    match body.get("state").and_then(|s| s.as_str()) {
        Some("linked") => {
            let token = body
                .get("token")
                .and_then(|t| t.as_str())
                .ok_or("the tone browser said linked and sent no credential")?;
            let account = body
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("your account");
            Ok(Some(Linked::In {
                token: token.to_owned(),
                account: account.to_owned(),
            }))
        }
        Some("pending") => Ok(None),
        _ => Ok(Some(Linked::GaveUp(
            "that code expired. Ask for a fresh one".to_owned(),
        ))),
    }
}

fn agent() -> String {
    format!("TonePush {VERSION} ({})", std::env::consts::OS)
}

/// The site's answer, as JSON. Anything unreadable is reported as such rather
/// than left to look like a refusal.
fn read_json(response: &mut ureq::http::Response<ureq::Body>) -> Result<serde_json::Value, String> {
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("the tone browser answered nothing readable: {e}"))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("the tone browser answered something unreadable: {e}"))
}

/// What the site is told about a tone being published.
///
/// The library already asks for all of this, which is the point: a row that has
/// been filled in is a listing, and one that has not cannot be published until
/// it is. Nothing here is invented on anybody's behalf.
#[derive(Debug, Default)]
pub struct Publishing {
    pub name: String,
    pub artist: String,
    pub song: String,
    pub part: String,
    pub description: String,
    pub creator_name: String,
    /// The portable copy. Its hash is what the library matches against, so this
    /// is the `.hlx` and never the pedal's own bytes.
    pub hlx: Vec<u8>,
    pub filename: String,
}

/// Put one tone on the site, with its file.
pub fn publish(token: &str, tone: &Publishing) -> Result<String, String> {
    let kind = if tone.song.is_empty() {
        "original"
    } else {
        "song"
    };
    let mut form = Multipart::new();
    form.field("creator_name", &tone.creator_name);
    form.field("tone[name]", &tone.name);
    form.field("tone[kind]", kind);
    form.field("tone[artist_name]", &tone.artist);
    form.field("tone[song]", &tone.song);
    form.field("tone[part]", &tone.part);
    form.field("tone[description]", &tone.description);
    form.field("tone[device_name]", "HX Stomp");
    form.file("tone[preset]", &tone.filename, &tone.hlx);

    // Keep the response body on validation errors. In particular, the site
    // answers a duplicate file with 422 plus a precise JSON error; treating
    // the status as a transport failure threw that useful answer away.
    let http = ureq::config::Config::builder()
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut response = http
        .post(tones())
        .header("User-Agent", agent())
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", form.content_type())
        .send(form.finish())
        .map_err(|e| format!("the tone browser refused it: {e}"))?;
    let status = response.status().as_u16();
    let body = read_json(&mut response)?;
    publish_answer(status, &body, &tone.name)
}

/// Interpret a publish response, including the useful body of a non-2xx one.
fn publish_answer(status: u16, body: &serde_json::Value, fallback: &str) -> Result<String, String> {
    let errors: Vec<&str> = body
        .get("errors")
        .and_then(|e| e.as_array())
        .into_iter()
        .flatten()
        .filter_map(|e| e.as_str())
        .collect();
    // The desired end state already exists. Report this as success so the
    // caller fills the cloud instead of presenting a failed upload.
    if errors
        .iter()
        .any(|error| error.eq_ignore_ascii_case("File sha256 has already been taken"))
    {
        return Ok(fallback.to_owned());
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    if !(200..300).contains(&status) {
        let said = body
            .get("error")
            .or_else(|| body.get("message"))
            .and_then(|value| value.as_str())
            .filter(|said| !said.is_empty())
            .map_or_else(
                || format!("the tone browser refused it (HTTP {status})"),
                str::to_owned,
            );
        return Err(said);
    }
    Ok(body
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or(fallback)
        .to_owned())
}

/// A multipart body, built by hand.
///
/// A file has to go up as multipart - that is what the site's form takes - and
/// that is thirty lines of joining bytes together. A crate for it would be a
/// dependency for thirty lines.
struct Multipart {
    boundary: String,
    body: Vec<u8>,
}

impl Multipart {
    fn new() -> Self {
        // Unique enough: it only has to not appear in the parts, and the parts
        // are a preset file and some short strings.
        let boundary = format!(
            "----TonePush{:x}",
            std::process::id() as u64 * 2_654_435_761
        );
        Multipart {
            boundary,
            body: Vec::new(),
        }
    }

    fn field(&mut self, name: &str, value: &str) {
        self.body.extend_from_slice(
            format!(
                "--{}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n",
                self.boundary
            )
            .as_bytes(),
        );
    }

    fn file(&mut self, name: &str, filename: &str, bytes: &[u8]) {
        self.body.extend_from_slice(
            format!(
                "--{}\r\nContent-Disposition: form-data; name=\"{name}\"; \
                 filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
                self.boundary
            )
            .as_bytes(),
        );
        self.body.extend_from_slice(bytes);
        self.body.extend_from_slice(b"\r\n");
    }

    fn content_type(&self) -> String {
        format!("multipart/form-data; boundary={}", self.boundary)
    }

    fn finish(mut self) -> Vec<u8> {
        self.body
            .extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        self.body
    }
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
    fn an_index_that_lists_hashes_needs_no_second_request() {
        let tone = serde_json::json!({ "id": 1, "file_sha256s": ["A".repeat(64)] });
        assert_eq!(summary_hashes(&tone), Some(vec!["a".repeat(64)]));
    }

    /// The distinction the fallback turns on: a tone with nothing published is
    /// an empty list, and a site that does not carry them at all is no field.
    #[test]
    fn no_field_and_an_empty_field_mean_different_things() {
        let older = serde_json::json!({ "id": 1 });
        assert_eq!(summary_hashes(&older), None, "ask this one for its details");

        let none_published = serde_json::json!({ "id": 1, "file_sha256s": [] });
        assert_eq!(
            summary_hashes(&none_published),
            Some(Vec::new()),
            "this one has answered; asking again would be a wasted request"
        );
    }

    #[test]
    fn a_tone_with_no_implementations_is_not_an_error() {
        assert!(file_hashes(&serde_json::json!({ "id": 1 })).is_empty());
        assert!(file_hashes(&serde_json::json!({ "implementations": [] })).is_empty());
    }

    #[test]
    fn a_duplicate_file_is_already_published_not_a_failed_publish() {
        let body = serde_json::json!({
            "errors": ["File sha256 has already been taken"]
        });
        assert_eq!(publish_answer(422, &body, "My tone").unwrap(), "My tone");
    }

    #[test]
    fn a_real_publish_error_is_preserved() {
        let body = serde_json::json!({ "errors": ["Name can't be blank"] });
        assert_eq!(
            publish_answer(422, &body, "My tone").unwrap_err(),
            "Name can't be blank"
        );
    }
}
