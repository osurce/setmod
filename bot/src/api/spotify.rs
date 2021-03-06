//! Spotify API helpers.

use crate::{oauth2, prelude::*};
use bytes::Bytes;
use failure::Error;
use reqwest::{
    header,
    r#async::{Client, Decoder},
    Method, StatusCode, Url,
};
use rspotify::spotify::model::search;
pub use rspotify::spotify::{
    model::{
        artist::SimplifiedArtist,
        context::FullPlayingContext,
        device::Device,
        page::Page,
        playlist::{FullPlaylist, SimplifiedPlaylist},
        track::{FullTrack, SavedTrack},
    },
    senum::DeviceType,
};
use std::{
    mem,
    pin::Pin,
    task::{Context, Poll},
};

const API_URL: &'static str = "https://api.spotify.com/v1";

/// API integration.
#[derive(Clone, Debug)]
pub struct Spotify {
    client: Client,
    api_url: Url,
    pub token: oauth2::SyncToken,
}

impl Spotify {
    /// Create a new API integration.
    pub fn new(token: oauth2::SyncToken) -> Result<Spotify, Error> {
        Ok(Spotify {
            client: Client::new(),
            api_url: str::parse::<Url>(API_URL)?,
            token,
        })
    }

    /// Get request against API.
    fn request(&self, method: Method, path: &[&str]) -> RequestBuilder {
        let mut url = self.api_url.clone();
        url.path_segments_mut().expect("bad base").extend(path);

        RequestBuilder {
            token: self.token.clone(),
            client: self.client.clone(),
            url,
            method,
            headers: Vec::new(),
            body: None,
        }
    }

    /// Get my playlists.
    pub async fn playlist(&self, id: String) -> Result<FullPlaylist, Error> {
        self.request(Method::GET, &["playlists", id.as_str()])
            .execute()
            .await
    }

    /// Get my devices.
    pub async fn my_player_devices(&self) -> Result<Vec<Device>, Error> {
        let r = self
            .request(Method::GET, &["me", "player", "devices"])
            .execute::<Response>()
            .await?;

        return Ok(r.devices);

        #[derive(serde::Deserialize)]
        struct Response {
            devices: Vec<Device>,
        }
    }

    /// Set player volume.
    pub async fn me_player_volume(
        &self,
        device_id: Option<String>,
        volume: f32,
    ) -> Result<bool, Error> {
        let volume = u32::min(100, (volume * 100f32).round() as u32).to_string();

        self.request(Method::PUT, &["me", "player", "volume"])
            .optional_query_param("device_id", device_id)
            .query_param("volume_percent", &volume)
            .header(header::ACCEPT, "application/json")
            .header(header::CONTENT_LENGTH, "0")
            .execute_empty_not_found()
            .await
    }

    /// Start playing a track.
    pub async fn me_player_pause(&self, device_id: Option<String>) -> Result<bool, Error> {
        self.request(Method::PUT, &["me", "player", "pause"])
            .optional_query_param("device_id", device_id)
            .header(header::CONTENT_LENGTH, "0")
            .header(header::ACCEPT, "application/json")
            .execute_empty_not_found()
            .await
    }

    /// Information on the current playback.
    pub async fn me_player(&self) -> Result<Option<FullPlayingContext>, Error> {
        self.request(Method::GET, &["me", "player"])
            .execute_optional()
            .await
    }

    /// Start playing a track.
    pub async fn me_player_play(
        &self,
        device_id: Option<String>,
        track_uri: Option<String>,
        position_ms: Option<u64>,
    ) -> Result<bool, Error> {
        let request = Request {
            uris: track_uri.into_iter().collect(),
            position_ms,
        };

        let r = self
            .request(Method::PUT, &["me", "player", "play"])
            .optional_query_param("device_id", device_id)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json");

        let body = Bytes::from(serde_json::to_vec(&request)?);
        return r.body(body).execute_empty_not_found().await;

        #[derive(serde::Serialize)]
        struct Request {
            #[serde(skip_serializing_if = "Vec::is_empty")]
            uris: Vec<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            position_ms: Option<u64>,
        }
    }

    /// Get my playlists.
    pub async fn my_playlists(&self) -> Result<Page<SimplifiedPlaylist>, Error> {
        self.request(Method::GET, &["me", "playlists"])
            .execute()
            .await
    }

    /// Get my songs.
    pub async fn my_tracks(&self) -> Result<Page<SavedTrack>, Error> {
        self.request(Method::GET, &["me", "tracks"]).execute().await
    }

    /// Get my songs.
    pub fn my_tracks_stream(&self) -> PageStream<SavedTrack> {
        self.page_stream(self.request(Method::GET, &["me", "tracks"]).execute())
    }

    /// Get the full track by ID.
    pub async fn track(&self, id: String) -> Result<FullTrack, Error> {
        self.request(Method::GET, &["tracks", id.as_str()])
            .execute()
            .await
    }

    /// Search for tracks.
    pub async fn search_track(&self, q: String) -> Result<Page<FullTrack>, Error> {
        self.request(Method::GET, &["search"])
            .query_param("type", "track")
            .query_param("q", &q)
            .execute::<search::SearchTracks>()
            .await
            .map(|r| r.tracks)
    }

    /// Convert a page object into a stream.
    pub fn page_as_stream<T>(&self, page: Page<T>) -> PageStream<T>
    where
        T: 'static + Send + serde::de::DeserializeOwned,
    {
        self.page_stream(future::ok(page))
    }

    /// Create a streamed page request.
    fn page_stream<T>(
        &self,
        future: impl Future<Output = Result<Page<T>, Error>> + Send + 'static,
    ) -> PageStream<T> {
        PageStream {
            client: self.client.clone(),
            token: self.token.clone(),
            next: Some(future.boxed()),
        }
    }
}

pub struct PageStream<T> {
    client: Client,
    token: oauth2::SyncToken,
    next: Option<future::BoxFuture<'static, Result<Page<T>, Error>>>,
}

impl<T> PageStream<T>
where
    T: serde::de::DeserializeOwned,
{
    /// Get the next page for a type.
    pub fn next_page(&self, url: Url) -> impl Future<Output = Result<Page<T>, Error>> {
        let client = self.client.clone();
        let token = self.token.clone();

        let request = RequestBuilder {
            token,
            client,
            url,
            method: Method::GET,
            headers: Vec::new(),
            body: None,
        };

        request.execute()
    }
}

impl<T> TryStream for PageStream<T>
where
    T: 'static + Send + serde::de::DeserializeOwned,
{
    type Ok = Vec<T>;
    type Error = Error;

    fn try_poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
    ) -> Poll<Option<Result<Self::Ok, Self::Error>>> {
        let future = match self.next.as_mut() {
            Some(future) => future,
            None => return Poll::Ready(None),
        };

        if let Poll::Ready(page) = future.as_mut().poll(cx)? {
            self.as_mut().next = match page.next.map(|s| str::parse(s.as_str())).transpose()? {
                Some(next) => Some(self.next_page(next).boxed()),
                None => None,
            };

            return Poll::Ready(Some(Ok(page.items)));
        }

        Poll::Pending
    }
}

struct RequestBuilder {
    token: oauth2::SyncToken,
    client: Client,
    url: Url,
    method: Method,
    headers: Vec<(header::HeaderName, String)>,
    body: Option<Bytes>,
}

impl RequestBuilder {
    /// Execute the request requiring content to be returned.
    pub async fn execute<T>(self) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        match self.execute_optional().await? {
            Some(body) => Ok(body),
            None => Err(failure::format_err!("got empty response from server")),
        }
    }

    /// Execute the request, taking into account that the server might return 204 NO CONTENT, and treat it as
    /// `Option::None`
    pub async fn execute_optional<T>(self) -> Result<Option<T>, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut req = self.client.request(self.method, self.url);

        if let Some(body) = self.body {
            req = req.body(body);
        }

        for (key, value) in self.headers {
            req = req.header(key, value);
        }

        let access_token = self.token.read()?.access_token().to_string();
        let req = req.header(header::AUTHORIZATION, format!("Bearer {}", access_token));

        let mut res = req.send().compat().await?;
        let body = mem::replace(res.body_mut(), Decoder::empty());
        let body = body.compat().try_concat().await?;

        let status = res.status();

        if status == StatusCode::UNAUTHORIZED {
            self.token.force_refresh()?;
        }

        if !status.is_success() {
            failure::bail!(
                "bad response: {}: {}",
                status,
                String::from_utf8_lossy(body.as_ref())
            );
        }

        if status == StatusCode::NO_CONTENT {
            return Ok(None);
        }

        if log::log_enabled!(log::Level::Trace) {
            let response = String::from_utf8_lossy(body.as_ref());
            log::trace!("response: {}", response);
        }

        match serde_json::from_slice(body.as_ref()) {
            Ok(body) => Ok(Some(body)),
            Err(e) => {
                log::trace!(
                    "failed to deserialize: {}: {}: {}",
                    status,
                    e,
                    String::from_utf8_lossy(body.as_ref())
                );
                Err(e.into())
            }
        }
    }

    /// Execute the request, expecting nothing back.
    pub async fn execute_empty_not_found(self) -> Result<bool, Error> {
        let RequestBuilder {
            token,
            client,
            url,
            method,
            headers,
            body,
        } = self;

        let access_token = token.read()?.access_token().to_string();

        let mut r = client.request(method, url);

        if let Some(body) = body {
            r = r.body(body);
        }

        for (key, value) in headers {
            r = r.header(key, value);
        }

        let request = r.header(header::AUTHORIZATION, format!("Bearer {}", access_token));

        let mut res = request.send().compat().await?;
        let body = mem::replace(res.body_mut(), Decoder::empty());
        let body = body.compat().try_concat().await?;

        let status = res.status();

        if status == StatusCode::NOT_FOUND {
            log::trace!("not found: {}", String::from_utf8_lossy(body.as_ref()));
            return Ok(false);
        }

        if !status.is_success() {
            failure::bail!(
                "bad response: {}: {}",
                status,
                String::from_utf8_lossy(body.as_ref())
            );
        }

        if log::log_enabled!(log::Level::Trace) {
            log::trace!("response: {}", String::from_utf8_lossy(body.as_ref()));
        }

        Ok(true)
    }

    /// Add a body to the request.
    pub fn body(mut self, body: Bytes) -> Self {
        self.body = Some(body);
        self
    }

    /// Push a header.
    pub fn header(mut self, key: header::HeaderName, value: &str) -> Self {
        self.headers.push((key, value.to_string()));
        self
    }

    /// Add a query parameter.
    pub fn query_param(mut self, key: &str, value: &str) -> Self {
        self.url.query_pairs_mut().append_pair(key, value);
        self
    }

    /// Add a query parameter.
    pub fn optional_query_param(mut self, key: &str, value: Option<String>) -> Self {
        if let Some(value) = value {
            self.url.query_pairs_mut().append_pair(key, value.as_str());
        }

        self
    }
}
