//! The TonePush web API: Songs are musical ideas and Tones are playable,
//! device-native presets that belong to them.
//!
//! Public discovery is deliberately typed at this boundary. The library only
//! needs Song file hashes today, while the same client also exposes Song
//! details and individual Tone downloads for the browser/install UI. Publishing
//! mirrors the two resources on the server: create a Song, then add its first
//! Tone. If the second request fails the error says that the Song was created;
//! there is no delete call and therefore no pretend rollback.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::mpsc::{channel, Receiver};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::update::VERSION;

const SITE: &str = "https://tonepush.rocks";
const PAGES: u32 = 100;

/// Where the web application lives. `TONEPUSH_SITE` is useful when exercising
/// the editor against a local Rails server.
pub fn site() -> String {
    std::env::var("TONEPUSH_SITE").unwrap_or_else(|_| SITE.to_owned())
}

fn agent_name() -> String {
    format!("TonePush {VERSION} ({})", std::env::consts::OS)
}

fn api_agent() -> ureq::Agent {
    ureq::config::Config::builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
}

/// A catalog song by an Artist or an original musical idea.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SongKind {
    Song,
    Original,
}

/// One row returned by `GET /api/v1/songs` and embedded in Tone details.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SongSummary {
    pub id: i64,
    pub title: String,
    pub kind: SongKind,
    pub artist: Option<String>,
    pub part: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    pub tuning: Option<String>,
    pub guitar_type: Option<String>,
    pub pickup_type: Option<String>,
    pub pickup_electronics: Option<String>,
    pub tone_count: u64,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default)]
    pub file_sha256s: Vec<String>,
}

/// One Song together with the playable Tones a person may choose from.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SongDetails {
    #[serde(flatten)]
    pub summary: SongSummary,
    #[serde(default)]
    pub tones: Vec<ToneDetails>,
}

/// The device a Tone can be installed on.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DeviceSummary {
    pub id: i64,
    pub name: String,
    pub slug: String,
    pub family: Option<String>,
    pub manufacturer: String,
    #[serde(default)]
    pub capabilities: serde_json::Value,
    #[serde(default)]
    pub artifact_extensions: Vec<String>,
}

/// The fields needed to present a playable Tone in a Song detail view.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ToneSummary {
    pub id: i64,
    pub song_id: i64,
    pub name: String,
    pub device: DeviceSummary,
    pub creator: Option<String>,
    pub description: Option<String>,
    pub state: String,
    pub availability: String,
    pub source_kind: String,
    pub firmware_version: Option<String>,
    pub minimum_firmware_version: Option<String>,
    pub parser_version: Option<String>,
    pub installs_count: u64,
    pub saves_count: u64,
    pub remix_count: u64,
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub signal_chain: Vec<serde_json::Value>,
}

/// Where a playable Tone comes from. Native Tones have an artifact path;
/// externally indexed Tones lead to their original source instead.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ToneDownload {
    pub artifact: Option<String>,
    pub external: Option<String>,
}

/// The complete response from `GET /api/v1/tones/:id`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ToneDetails {
    #[serde(flatten)]
    pub summary: ToneSummary,
    pub file_sha256: Option<String>,
    #[serde(default)]
    pub parsed_metadata: serde_json::Value,
    #[serde(default)]
    pub dependencies: Vec<serde_json::Value>,
    #[serde(default)]
    pub audio_previews: Vec<serde_json::Value>,
    pub download: Option<ToneDownload>,
    /// Present on the individual Tone endpoint. A Tone embedded in its own
    /// Song details does not repeat the parent Song.
    pub song: Option<SongSummary>,
}

/// Facets accepted by Song search. Empty values are not sent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SongSearch {
    pub q: Option<String>,
    pub artist: Option<String>,
    pub device: Option<String>,
    pub output_target: Option<String>,
    pub genre: Option<String>,
    pub page: u32,
}

#[derive(Debug, Deserialize)]
struct SongsResponse {
    songs: Vec<SongSummary>,
}

/// A client rooted at one TonePush web deployment.
#[derive(Clone)]
pub struct CloudClient {
    base: String,
    http: ureq::Agent,
}

impl CloudClient {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_owned(),
            http: api_agent(),
        }
    }

    fn songs_url(&self) -> String {
        format!("{}/api/v1/songs", self.base)
    }

    /// Search musical ideas. The returned rows are Songs, not installable
    /// presets; call [`Self::song`] and let the person choose one of its Tones.
    pub fn search_songs(&self, search: &SongSearch) -> Result<Vec<SongSummary>, String> {
        let mut request = self
            .http
            .get(self.songs_url())
            .header("User-Agent", agent_name())
            .header("Accept", "application/json")
            .query("page", search.page.max(1).to_string());
        for (key, value) in [
            ("q", search.q.as_deref()),
            ("artist", search.artist.as_deref()),
            ("device", search.device.as_deref()),
            ("output_target", search.output_target.as_deref()),
            ("genre", search.genre.as_deref()),
        ] {
            if let Some(value) = value.filter(|value| !value.is_empty()) {
                request = request.query(key, value);
            }
        }
        let response = request
            .call()
            .map_err(|error| format!("the Song catalog did not answer: {error}"))?;
        decode(response).map(|response: SongsResponse| response.songs)
    }

    /// Fetch one musical idea and the playable Tones belonging to it.
    pub fn song(&self, id: i64) -> Result<SongDetails, String> {
        let response = self
            .http
            .get(format!("{}/api/v1/songs/{id}", self.base))
            .header("User-Agent", agent_name())
            .header("Accept", "application/json")
            .call()
            .map_err(|error| format!("the Song catalog did not answer: {error}"))?;
        decode(response)
    }

    /// Fetch one playable, device-native Tone.
    pub fn tone(&self, id: i64) -> Result<ToneDetails, String> {
        let response = self
            .http
            .get(format!("{}/api/v1/tones/{id}", self.base))
            .header("User-Agent", agent_name())
            .header("Accept", "application/json")
            .call()
            .map_err(|error| format!("the Tone catalog did not answer: {error}"))?;
        decode(response)
    }

    /// Resolve a Tone's download without pretending an external catalog entry
    /// is a native artifact. Callers open [`ToneDelivery::External`] in the
    /// browser and install [`ToneDelivery::Artifact`] into the local library.
    pub fn download(&self, tone: &ToneDetails) -> Result<ToneDelivery, String> {
        let Some(location) = &tone.download else {
            return Err(format!("{} has no downloadable preset", tone.summary.name));
        };
        if let Some(url) = &location.external {
            return Ok(ToneDelivery::External(url.clone()));
        }
        let Some(path) = &location.artifact else {
            return Err(format!("{} has no downloadable preset", tone.summary.name));
        };
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.clone()
        } else {
            format!("{}{}", self.base, path)
        };
        let mut response = self
            .http
            .get(url)
            .header("User-Agent", agent_name())
            .call()
            .map_err(|error| format!("the Tone artifact did not answer: {error}"))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let body = response.body_mut().read_to_string().unwrap_or_default();
            return Err(api_error(status, &body));
        }
        let bytes = response
            .body_mut()
            .read_to_vec()
            .map_err(|error| format!("the Tone artifact was unreadable: {error}"))?;
        Ok(ToneDelivery::Artifact(bytes))
    }

    /// Create a musical idea. This operation never uploads a preset.
    pub fn create_song(
        &self,
        token: &str,
        request: &CreateSongRequest,
    ) -> Result<SongDetails, String> {
        let body = serde_json::to_vec(request)
            .map_err(|error| format!("the Song could not be encoded: {error}"))?;
        let response = self
            .http
            .post(self.songs_url())
            .header("User-Agent", agent_name())
            .header("Accept", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .content_type("application/json")
            .send(body)
            .map_err(|error| format!("the Song catalog did not answer: {error}"))?;
        decode(response)
    }

    /// Add one playable Tone to an existing Song.
    pub fn create_tone(
        &self,
        token: &str,
        song_id: i64,
        request: &CreateToneRequest,
    ) -> Result<ToneDetails, String> {
        let form = request.multipart();
        let response = self
            .http
            .post(format!("{}/api/v1/songs/{song_id}/tones", self.base))
            .header("User-Agent", agent_name())
            .header("Accept", "application/json")
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", form.content_type())
            .send(form.finish())
            .map_err(|error| format!("the Tone catalog did not answer: {error}"))?;
        decode(response)
    }

    /// Publish through the resourceful two-step contract. An existing Song
    /// goes straight to Tone creation; a new one is created first.
    pub fn publish(
        &self,
        token: &str,
        request: &PublishRequest,
    ) -> Result<ToneDetails, PublishError> {
        let (song_id, created) = match &request.song {
            PublishSong::Existing(id) => (*id, None),
            PublishSong::New(song) => {
                let created = self
                    .create_song(token, song)
                    .map_err(PublishError::CreatingSong)?;
                (created.summary.id, Some(created.summary.title))
            }
        };
        self.create_tone(token, song_id, &request.tone)
            .map_err(|reason| PublishError::CreatingTone {
                song_id,
                created_song: created,
                reason,
            })
    }
}

/// A resolved Tone download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToneDelivery {
    Artifact(Vec<u8>),
    External(String),
}

/// The JSON body for creating one Song.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateSongRequest {
    pub creator_name: String,
    pub song: NewSong,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewSong {
    pub title: String,
    pub kind: SongKind,
    pub artist_name: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub genre_ids: Vec<i64>,
}

/// The portable preset attached to a new Tone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetUpload {
    pub filename: String,
    pub bytes: Vec<u8>,
}

/// The multipart body for adding a Tone to a Song.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CreateToneRequest {
    pub creator_name: String,
    pub tone: NewTone,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NewTone {
    pub name: String,
    pub description: Option<String>,
    pub part: Option<String>,
    pub tuning: Option<String>,
    pub guitar_type: Option<String>,
    pub pickup_type: Option<String>,
    pub pickup_electronics: Option<String>,
    pub device_id: Option<i64>,
    pub device_name: Option<String>,
    pub firmware_version: Option<String>,
    pub parser_version: Option<String>,
    pub output_target: Option<String>,
    pub chain_content: Option<String>,
    pub character: Option<String>,
    pub blocks: Vec<serde_json::Value>,
    pub parsed_metadata: serde_json::Value,
    pub preset: Option<PresetUpload>,
}

impl CreateToneRequest {
    fn multipart(&self) -> Multipart {
        let mut form = Multipart::new();
        if !self.creator_name.is_empty() {
            form.field("creator_name", &self.creator_name);
        }
        form.field("tone[name]", &self.tone.name);
        for (field, value) in [
            ("description", self.tone.description.as_deref()),
            ("part", self.tone.part.as_deref()),
            ("tuning", self.tone.tuning.as_deref()),
            ("guitar_type", self.tone.guitar_type.as_deref()),
            ("pickup_type", self.tone.pickup_type.as_deref()),
            (
                "pickup_electronics",
                self.tone.pickup_electronics.as_deref(),
            ),
            ("device_name", self.tone.device_name.as_deref()),
            ("firmware_version", self.tone.firmware_version.as_deref()),
            ("parser_version", self.tone.parser_version.as_deref()),
            ("output_target", self.tone.output_target.as_deref()),
            ("chain_content", self.tone.chain_content.as_deref()),
            ("character", self.tone.character.as_deref()),
        ] {
            if let Some(value) = value {
                form.field(&format!("tone[{field}]"), value);
            }
        }
        if let Some(id) = self.tone.device_id {
            form.field("tone[device_id]", &id.to_string());
        }
        form.object_array("tone[blocks]", &self.tone.blocks);
        if !self.tone.parsed_metadata.is_null()
            && self
                .tone
                .parsed_metadata
                .as_object()
                .is_none_or(|object| !object.is_empty())
        {
            form.json("tone[parsed_metadata]", &self.tone.parsed_metadata);
        }
        if let Some(preset) = &self.tone.preset {
            form.file("tone[preset]", &preset.filename, &preset.bytes);
        }
        form
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PublishRequest {
    pub song: PublishSong,
    pub tone: CreateToneRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishSong {
    New(CreateSongRequest),
    Existing(i64),
}

/// Which of the two publishing operations failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishError {
    CreatingSong(String),
    CreatingTone {
        song_id: i64,
        created_song: Option<String>,
        reason: String,
    },
}

impl fmt::Display for PublishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreatingSong(reason) => write!(formatter, "the Song was not created: {reason}"),
            Self::CreatingTone {
                song_id,
                created_song: Some(title),
                reason,
            } => write!(
                formatter,
                "Song “{title}” was created as #{song_id}, but its Tone was not published: \
                 {reason}. The empty Song remains on TonePush"
            ),
            Self::CreatingTone {
                song_id,
                created_song: None,
                reason,
            } => write!(
                formatter,
                "the Tone was not added to Song #{song_id}: {reason}"
            ),
        }
    }
}

/// Ask the Song index what native files are already published, off the UI
/// thread. Failure yields no value; a successful empty catalog yields an empty
/// set so the library can still offer its first publish action.
pub fn published() -> Receiver<BTreeSet<String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        if let Ok(hashes) = fetch_published(&CloudClient::new(site())) {
            let _ = tx.send(hashes);
        }
    });
    rx
}

fn fetch_published(client: &CloudClient) -> Result<BTreeSet<String>, String> {
    let mut found = BTreeSet::new();
    for page in 1..=PAGES {
        let songs = client.search_songs(&SongSearch {
            page,
            ..Default::default()
        })?;
        if songs.is_empty() {
            break;
        }
        for song in songs {
            found.extend(
                song.file_sha256s
                    .into_iter()
                    .filter(|hash| hash.len() == 64)
                    .map(|hash| hash.to_ascii_lowercase()),
            );
        }
    }
    Ok(found)
}

/// Open the published Tone represented by a portable preset hash. The web app
/// resolves the hash and redirects to the Tone's canonical Song-nested page,
/// so the editor never has to guess Artist or Song slugs.
pub fn tone_url(file_sha256: &str) -> String {
    format!(
        "{}/tones/files/{}",
        site(),
        file_sha256.trim().to_ascii_lowercase()
    )
}

/// Signing this computer in remains the existing pairing flow.
fn pairings() -> String {
    format!("{}/api/v1/pairings", site())
}

#[derive(Debug, Clone)]
pub struct Pairing {
    pub code: String,
    pub url: String,
}

pub enum Linked {
    In { token: String, account: String },
    GaveUp(String),
}

pub fn start_pairing() -> Result<Pairing, String> {
    let mut response = ureq::post(pairings())
        .header("User-Agent", agent_name())
        .header("Accept", "application/json")
        .send_empty()
        .map_err(|error| format!("TonePush did not answer the pairing request: {error}"))?;
    let body: serde_json::Value = decode_body(&mut response)?;
    let (Some(code), Some(url)) = (
        body.get("code").and_then(|code| code.as_str()),
        body.get("url").and_then(|url| url.as_str()),
    ) else {
        return Err("TonePush did not offer a pairing code".to_owned());
    };
    Ok(Pairing {
        code: code.to_owned(),
        url: url.to_owned(),
    })
}

pub fn poll_pairing(code: &str) -> Result<Option<Linked>, String> {
    let mut response = ureq::get(format!("{}/{code}", pairings()))
        .header("User-Agent", agent_name())
        .header("Accept", "application/json")
        .call()
        .map_err(|error| format!("TonePush stopped answering the pairing request: {error}"))?;
    let body: serde_json::Value = decode_body(&mut response)?;
    match body.get("state").and_then(|state| state.as_str()) {
        Some("linked") => {
            let token = body
                .get("token")
                .and_then(|token| token.as_str())
                .ok_or("TonePush paired without returning a session token")?;
            let account = body
                .get("name")
                .and_then(|name| name.as_str())
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

/// Publish a new Song and Tone against the configured site.
pub fn publish(token: &str, request: &PublishRequest) -> Result<ToneDetails, PublishError> {
    CloudClient::new(site()).publish(token, request)
}

fn decode<T: DeserializeOwned>(
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<T, String> {
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("TonePush answered nothing readable: {error}"))?;
    if !(200..300).contains(&status) {
        return Err(api_error(status, &body));
    }
    serde_json::from_str(&body)
        .map_err(|error| format!("TonePush answered with invalid JSON: {error}"))
}

fn decode_body<T: DeserializeOwned>(
    response: &mut ureq::http::Response<ureq::Body>,
) -> Result<T, String> {
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("TonePush answered nothing readable: {error}"))?;
    serde_json::from_str(&body)
        .map_err(|error| format!("TonePush answered with invalid JSON: {error}"))
}

fn api_error(status: u16, body: &str) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let errors = parsed
        .as_ref()
        .and_then(|body| body.get("errors"))
        .and_then(|errors| errors.as_array())
        .into_iter()
        .flatten()
        .filter_map(|error| error.as_str())
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return errors.join("; ");
    }
    parsed
        .as_ref()
        .and_then(|body| body.get("error").or_else(|| body.get("message")))
        .and_then(|message| message.as_str())
        .filter(|message| !message.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("TonePush refused the request (HTTP {status})"))
}

/// A small multipart encoder with Rails-style nested field names.
struct Multipart {
    boundary: String,
    body: Vec<u8>,
}

impl Multipart {
    fn new() -> Self {
        let boundary = format!(
            "----TonePush{:x}",
            std::process::id() as u64 * 2_654_435_761
        );
        Self {
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

    fn object_array(&mut self, name: &str, values: &[serde_json::Value]) {
        for value in values {
            if let Some(object) = value.as_object() {
                for (key, value) in object {
                    self.json(&format!("{name}[][{key}]"), value);
                }
            }
        }
    }

    fn json(&mut self, name: &str, value: &serde_json::Value) {
        match value {
            serde_json::Value::Null => {}
            serde_json::Value::Bool(value) => self.field(name, &value.to_string()),
            serde_json::Value::Number(value) => self.field(name, &value.to_string()),
            serde_json::Value::String(value) => self.field(name, value),
            serde_json::Value::Array(values) => {
                for value in values {
                    self.json(&format!("{name}[]"), value);
                }
            }
            serde_json::Value::Object(values) => {
                for (key, value) in values {
                    self.json(&format!("{name}[{key}]"), value);
                }
            }
        }
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
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    fn song_json(id: i64, title: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "title": title,
            "kind": "original",
            "artist": null,
            "part": "Clean",
            "description": "Wide and clean",
            "tags": ["clean"],
            "genres": ["Ambient"],
            "tuning": "Standard",
            "guitar_type": "Stratocaster",
            "pickup_type": "single_coil",
            "pickup_electronics": "passive",
            "tone_count": 1,
            "devices": ["HX Stomp"],
            "file_sha256s": ["A".repeat(64)]
        })
    }

    fn tone_json(id: i64, song_id: i64, title: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "song_id": song_id,
            "name": "Wide HX",
            "device": {
                "id": 1,
                "name": "HX Stomp",
                "slug": "hx-stomp",
                "family": "Helix",
                "manufacturer": "Line 6",
                "capabilities": {},
                "artifact_extensions": ["hlx"]
            },
            "creator": "Public Name",
            "description": "The playable preset",
            "state": "published",
            "availability": "free",
            "source_kind": "native",
            "firmware_version": "3.80",
            "minimum_firmware_version": null,
            "parser_version": "0.4.3",
            "installs_count": 0,
            "saves_count": 0,
            "remix_count": 0,
            "parent_id": null,
            "signal_chain": [{"name": "Minotaur"}],
            "file_sha256": "b".repeat(64),
            "parsed_metadata": {},
            "dependencies": [],
            "audio_previews": [],
            "download": {"artifact": format!("/tones/{id}/artifact")},
            "song": song_json(song_id, title)
        })
    }

    struct StubServer {
        base: String,
        requests: Arc<Mutex<Vec<Vec<u8>>>>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl StubServer {
        fn start(responses: Vec<(u16, serde_json::Value)>) -> Self {
            Self::start_raw(
                responses
                    .into_iter()
                    .map(|(status, body)| (status, serde_json::to_vec(&body).unwrap()))
                    .collect(),
            )
        }

        fn start_raw(responses: Vec<(u16, Vec<u8>)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(Mutex::new(Vec::new()));
            let captured = requests.clone();
            let thread = std::thread::spawn(move || {
                for (status, body) in responses {
                    let (mut stream, _) = listener.accept().unwrap();
                    captured.lock().unwrap().push(read_request(&mut stream));
                    let reason = if (200..300).contains(&status) {
                        "OK"
                    } else {
                        "Unprocessable Entity"
                    };
                    write!(
                        stream,
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .unwrap();
                    stream.write_all(&body).unwrap();
                }
            });
            Self {
                base,
                requests,
                thread: Some(thread),
            }
        }

        fn finish(mut self) -> Vec<Vec<u8>> {
            self.thread.take().unwrap().join().unwrap();
            Arc::try_unwrap(self.requests)
                .unwrap()
                .into_inner()
                .unwrap()
        }
    }

    fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut request = Vec::new();
        let mut chunk = [0u8; 4096];
        let header_end = loop {
            let read = stream.read(&mut chunk).unwrap();
            assert!(read > 0);
            request.extend_from_slice(&chunk[..read]);
            if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while request.len() < header_end + length {
            let read = stream.read(&mut chunk).unwrap();
            assert!(read > 0);
            request.extend_from_slice(&chunk[..read]);
        }
        request
    }

    fn new_song() -> CreateSongRequest {
        CreateSongRequest {
            creator_name: "Public Name".into(),
            song: NewSong {
                title: "Wide Clean".into(),
                kind: SongKind::Original,
                artist_name: None,
                description: Some("A musical idea".into()),
                tags: vec!["clean".into(), "wide".into()],
                genre_ids: Vec::new(),
            },
        }
    }

    fn new_tone() -> CreateToneRequest {
        CreateToneRequest {
            creator_name: "Public Name".into(),
            tone: NewTone {
                name: "Wide HX".into(),
                part: Some("Clean".into()),
                device_name: Some("HX Stomp".into()),
                firmware_version: Some("3.80".into()),
                parser_version: Some(crate::update::VERSION.into()),
                blocks: vec![serde_json::json!({
                    "name": "Minotaur", "category": 1, "enabled": true, "path": 0
                })],
                parsed_metadata: serde_json::json!({"models_used": [101]}),
                preset: Some(PresetUpload {
                    filename: "wide.hlx".into(),
                    bytes: b"preset bytes".to_vec(),
                }),
                ..Default::default()
            },
        }
    }

    #[test]
    fn song_and_tone_wire_shapes_decode_separately() {
        let index: SongsResponse =
            serde_json::from_str(include_str!("../tests/fixtures/cloud/songs-index.json")).unwrap();
        let song: SongDetails =
            serde_json::from_str(include_str!("../tests/fixtures/cloud/song-details.json"))
                .unwrap();
        assert_eq!(index.songs[0], song.summary);
        assert_eq!(song.tones.len(), 2, "the UI must offer both playable Tones");
        assert_eq!(song.tones[0].summary.name, "Numb HX");
        assert!(song.tones[0]
            .download
            .as_ref()
            .and_then(|download| download.artifact.as_ref())
            .is_some());

        let tone: ToneDetails =
            serde_json::from_str(include_str!("../tests/fixtures/cloud/tone-details.json"))
                .unwrap();
        assert_eq!(tone.summary.song_id, song.summary.id);
        assert_eq!(
            tone.song.as_ref().map(|song| song.title.as_str()),
            Some(song.summary.title.as_str())
        );
        assert_eq!(tone.file_sha256, Some("a".repeat(64)));
    }

    #[test]
    fn song_index_hashes_are_normalized_for_library_matching() {
        let server = StubServer::start(vec![
            (
                200,
                serde_json::json!({"songs": [song_json(12, "Wide Clean")]}),
            ),
            (200, serde_json::json!({"songs": []})),
        ]);
        let client = CloudClient::new(&server.base);
        assert_eq!(
            fetch_published(&client).unwrap(),
            BTreeSet::from(["a".repeat(64)])
        );
        let requests = server.finish();
        assert!(String::from_utf8_lossy(&requests[0]).starts_with("GET /api/v1/songs?page=1 "));
    }

    #[test]
    fn search_sends_every_supported_song_facet() {
        let server = StubServer::start(vec![(200, serde_json::json!({"songs": []}))]);
        let client = CloudClient::new(&server.base);
        client
            .search_songs(&SongSearch {
                q: Some("wide clean".into()),
                artist: Some("7".into()),
                device: Some("2".into()),
                output_target: Some("frfr_pa".into()),
                genre: Some("3".into()),
                page: 4,
            })
            .unwrap();
        let request = server.finish().pop().unwrap();
        let request = String::from_utf8_lossy(&request);
        assert!(request.starts_with("GET /api/v1/songs?"));
        for pair in [
            "q=wide%20clean",
            "artist=7",
            "device=2",
            "output_target=frfr_pa",
            "genre=3",
            "page=4",
        ] {
            assert!(request.contains(pair), "missing {pair} in {request}");
        }
    }

    #[test]
    fn publishing_creates_the_song_then_adds_its_tone() {
        let mut song = song_json(12, "Wide Clean");
        song["tone_count"] = 0.into();
        song["tones"] = serde_json::json!([]);
        let server = StubServer::start(vec![(201, song), (201, tone_json(34, 12, "Wide Clean"))]);
        let client = CloudClient::new(&server.base);
        let published = client
            .publish(
                "session-token",
                &PublishRequest {
                    song: PublishSong::New(new_song()),
                    tone: new_tone(),
                },
            )
            .unwrap();
        assert_eq!(published.summary.id, 34);

        let requests = server.finish();
        let song_request = String::from_utf8_lossy(&requests[0]);
        assert!(song_request.starts_with("POST /api/v1/songs "));
        let song_body = song_request.split("\r\n\r\n").nth(1).unwrap();
        let song_body: serde_json::Value = serde_json::from_str(song_body).unwrap();
        assert_eq!(song_body["song"]["kind"], "original");
        assert_eq!(song_body["song"]["title"], "Wide Clean");

        let tone_request = String::from_utf8_lossy(&requests[1]);
        assert!(tone_request.starts_with("POST /api/v1/songs/12/tones "));
        for field in [
            "name=\"tone[name]\"",
            "name=\"tone[preset]\"",
            "name=\"tone[blocks][][name]\"",
            "name=\"tone[parsed_metadata][models_used][]\"",
        ] {
            assert!(tone_request.contains(field), "missing {field}");
        }
        for song_only in ["tone[kind]", "tone[artist_name]", "tone[tags]"] {
            assert!(!tone_request.contains(song_only), "sent {song_only}");
        }
    }

    #[test]
    fn adding_to_an_existing_song_skips_song_creation() {
        let server = StubServer::start(vec![(201, tone_json(34, 12, "Wide Clean"))]);
        let client = CloudClient::new(&server.base);
        client
            .publish(
                "session-token",
                &PublishRequest {
                    song: PublishSong::Existing(12),
                    tone: new_tone(),
                },
            )
            .unwrap();
        let requests = server.finish();
        assert_eq!(requests.len(), 1);
        assert!(String::from_utf8_lossy(&requests[0]).starts_with("POST /api/v1/songs/12/tones "));
    }

    #[test]
    fn a_failed_tone_reports_that_the_song_remains() {
        let mut song = song_json(12, "Wide Clean");
        song["tone_count"] = 0.into();
        song["tones"] = serde_json::json!([]);
        let server = StubServer::start(vec![
            (201, song),
            (422, serde_json::json!({"errors": ["Name can't be blank"]})),
        ]);
        let client = CloudClient::new(&server.base);
        let error = client
            .publish(
                "session-token",
                &PublishRequest {
                    song: PublishSong::New(new_song()),
                    tone: new_tone(),
                },
            )
            .unwrap_err();
        assert!(matches!(
            error,
            PublishError::CreatingTone {
                song_id: 12,
                created_song: Some(_),
                ..
            }
        ));
        assert!(error.to_string().contains("empty Song remains"));
        assert_eq!(server.finish().len(), 2, "no third request is attempted");
    }

    #[test]
    fn external_tones_return_their_source_without_fetching_it() {
        let mut json = tone_json(34, 12, "Wide Clean");
        json["source_kind"] = "external".into();
        json["file_sha256"] = serde_json::Value::Null;
        json["download"] = serde_json::json!({
            "external": "https://line6.com/customtone/tone/34/"
        });
        let tone: ToneDetails = serde_json::from_value(json).unwrap();
        let client = CloudClient::new("http://127.0.0.1:1");
        assert_eq!(
            client.download(&tone).unwrap(),
            ToneDelivery::External("https://line6.com/customtone/tone/34/".into())
        );
    }

    #[test]
    fn native_tones_download_the_artifact_selected_from_the_song() {
        let server = StubServer::start_raw(vec![(200, b"native preset".to_vec())]);
        let mut json: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/cloud/tone-details.json"))
                .unwrap();
        json["download"] = serde_json::json!({"artifact": "/tones/456/artifact"});
        let tone: ToneDetails = serde_json::from_value(json).unwrap();
        let client = CloudClient::new(&server.base);
        assert_eq!(
            client.download(&tone).unwrap(),
            ToneDelivery::Artifact(b"native preset".to_vec())
        );
        let request = server.finish().pop().unwrap();
        assert!(String::from_utf8_lossy(&request).starts_with("GET /tones/456/artifact "));
    }
}
