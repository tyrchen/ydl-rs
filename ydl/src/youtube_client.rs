// YouTube client simulation based on yt-dlp implementation
use crate::error::{YdlError, YdlResult};
use crate::types::{PlayerResponse, SubtitleTrack, SubtitleTrackType};
use reqwest::{
    Client,
    header::{HeaderMap, HeaderValue},
};
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, info, warn};

/// YouTube client types that work for subtitle extraction
#[derive(Debug, Clone)]
pub enum ClientType {
    Web,
    TvEmbedded,
    Ios,
    Android,
}

impl ClientType {
    fn client_name(&self) -> &str {
        match self {
            ClientType::Web => "WEB",
            ClientType::TvEmbedded => "TVHTML5_SIMPLY_EMBEDDED_PLAYER",
            ClientType::Ios => "IOS",
            ClientType::Android => "ANDROID",
        }
    }

    fn client_version(&self) -> &str {
        match self {
            ClientType::Web => "2.20240815.00.00",
            ClientType::TvEmbedded => "2.0",
            ClientType::Ios => "19.29.1",
            ClientType::Android => "19.29.37",
        }
    }

    // These API keys are public and can be found in: https://github.com/zerodytrash/YouTube-Internal-Clients/tree/main?tab=readme-ov-file#api-keys
    fn api_key(&self) -> &str {
        match self {
            ClientType::Web => "AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8",
            ClientType::TvEmbedded => "AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8",
            ClientType::Ios => "AIzaSyB-63vPrdThhKuerbB2N_l7Kwwcxj6yUA",
            ClientType::Android => "AIzaSyA8eiZmM1FaDVjRy-df2KTyQ_vz_yYM39w",
        }
    }

    fn user_agent(&self) -> &str {
        match self {
            ClientType::Web => {
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
            }
            ClientType::TvEmbedded => {
                "Mozilla/5.0 (PlayStation 4 5.55) AppleWebKit/601.2 (KHTML, like Gecko)"
            }
            ClientType::Ios => {
                "com.google.ios.youtube/19.29.1 (iPhone16,2; U; CPU iOS 17_5_1 like Mac OS X;)"
            }
            ClientType::Android => {
                "com.google.android.youtube/19.29.37 (Linux; U; Android 14; en_US; Pixel 7 Pro)"
            }
        }
    }
}

/// YouTube InnerTube client for API requests
pub struct InnerTubeClient {
    client: Client,
    client_type: ClientType,
}

impl InnerTubeClient {
    pub fn new(client_type: ClientType) -> YdlResult<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            HeaderValue::from_str(client_type.user_agent()).unwrap(),
        );
        headers.insert(reqwest::header::ACCEPT, HeaderValue::from_static("*/*"));
        headers.insert(
            reqwest::header::ACCEPT_LANGUAGE,
            HeaderValue::from_static("en-US,en;q=0.9"),
        );
        headers.insert(
            "X-Youtube-Client-Name",
            HeaderValue::from_str(match client_type {
                ClientType::Web => "1",
                ClientType::TvEmbedded => "85",
                ClientType::Ios => "5",
                ClientType::Android => "3",
            })
            .unwrap(),
        );
        headers.insert(
            "X-Youtube-Client-Version",
            HeaderValue::from_str(client_type.client_version()).unwrap(),
        );
        headers.insert(
            reqwest::header::ORIGIN,
            HeaderValue::from_static("https://www.youtube.com"),
        );

        let client = Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            client_type,
        })
    }

    /// Get player response using InnerTube API
    pub async fn get_player(&self, video_id: &str) -> YdlResult<PlayerResponse> {
        let url = format!(
            "https://www.youtube.com/youtubei/v1/player?key={}&prettyPrint=false",
            self.client_type.api_key()
        );

        let context = self.build_context();
        let body = json!({
            "videoId": video_id,
            "context": context,
            "contentCheckOk": true,
            "racyCheckOk": true,
        });

        debug!(
            "Requesting player data from {} client for video {}",
            self.client_type.client_name(),
            video_id
        );

        let response = self.client.post(&url).json(&body).send().await?;

        if !response.status().is_success() {
            warn!(
                "Failed to get player response from {} client: {}",
                self.client_type.client_name(),
                response.status()
            );
            return Err(YdlError::SubtitleDiscoveryError {
                message: format!("Failed to get player response: {}", response.status()),
            });
        }

        let player_response: PlayerResponse = response.json().await?;

        if let Some(_captions) = &player_response.captions {
            debug!(
                "Found captions in {} client response",
                self.client_type.client_name()
            );
        }

        Ok(player_response)
    }

    fn build_context(&self) -> serde_json::Value {
        let client = json!({
            "clientName": self.client_type.client_name(),
            "clientVersion": self.client_type.client_version(),
            "gl": "US",
            "hl": "en",
        });

        match self.client_type {
            ClientType::Web => {
                json!({
                    "client": client,
                })
            }
            ClientType::TvEmbedded => {
                json!({
                    "client": client,
                    "thirdParty": {
                        "embedUrl": "https://www.youtube.com/"
                    },
                })
            }
            ClientType::Ios | ClientType::Android => {
                json!({
                    "client": client,
                })
            }
        }
    }

    /// Extract subtitle tracks from player response
    pub fn extract_subtitle_tracks(
        &self,
        player_response: &PlayerResponse,
        _video_id: &str,
    ) -> Vec<SubtitleTrack> {
        let mut tracks = Vec::new();

        if let Some(captions) = &player_response.captions
            && let Some(tracklist) = &captions.player_captions_tracklist_renderer
            && let Some(caption_tracks) = &tracklist.caption_tracks
        {
            for track in caption_tracks {
                // The base_url is not optional in our types, so we can use it directly
                let base_url = &track.base_url;

                // Parse existing URL to check for required parameters
                let url = if base_url.contains("fmt=") {
                    base_url.clone()
                } else {
                    // Add format parameter for srv3 (XML format)
                    format!("{}&fmt=srv3", base_url)
                };

                let language_name = track
                    .name
                    .as_ref()
                    .and_then(|n| n.simple_text.as_deref())
                    .unwrap_or(&track.language_code);

                let track_type = if track.kind.as_deref() == Some("asr") {
                    SubtitleTrackType::AutoGenerated
                } else {
                    SubtitleTrackType::Manual
                };

                debug!(
                    "Found subtitle track: {} ({}) - {:?}",
                    language_name, track.language_code, track_type
                );

                let subtitle_track = SubtitleTrack::new(
                    track.language_code.clone(),
                    language_name.to_string(),
                    track_type,
                )
                .with_url(url)
                .with_translatable(track.is_translatable.unwrap_or(false));

                tracks.push(subtitle_track);
            }
        }

        tracks
    }
}

/// YouTube subtitle extractor using multiple client strategies
pub struct YouTubeSubtitleExtractor {
    clients: Vec<InnerTubeClient>,
}

impl YouTubeSubtitleExtractor {
    pub fn new() -> YdlResult<Self> {
        // Initialize multiple clients for fallback
        let clients = vec![
            InnerTubeClient::new(ClientType::TvEmbedded)?,
            InnerTubeClient::new(ClientType::Web)?,
            InnerTubeClient::new(ClientType::Ios)?,
            InnerTubeClient::new(ClientType::Android)?,
        ];

        Ok(Self { clients })
    }

    /// Discover subtitle tracks using multiple client strategies
    pub async fn discover_tracks(&self, video_id: &str) -> YdlResult<Vec<SubtitleTrack>> {
        info!(
            "Discovering subtitles for video {} using InnerTube API",
            video_id
        );

        // Try each client until we get subtitles
        for client in &self.clients {
            match client.get_player(video_id).await {
                Ok(player_response) => {
                    let tracks = client.extract_subtitle_tracks(&player_response, video_id);
                    if !tracks.is_empty() {
                        info!(
                            "Successfully found {} subtitle tracks using {} client",
                            tracks.len(),
                            client.client_type.client_name()
                        );
                        return Ok(tracks);
                    }
                }
                Err(e) => {
                    debug!(
                        "Failed to get subtitles with {} client: {}",
                        client.client_type.client_name(),
                        e
                    );
                }
            }
        }

        Err(YdlError::NoSubtitlesAvailable {
            video_id: video_id.to_string(),
        })
    }

    /// Download subtitle content from URL
    pub async fn download_content(&self, url: &str) -> YdlResult<String> {
        info!("Downloading subtitle from URL: {}", url);

        // Use the first client for downloading
        let response = self.clients[0].client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(YdlError::SubtitleDiscoveryError {
                message: format!("Failed to download subtitle: {}", response.status()),
            });
        }

        let content = response.text().await?;

        debug!("Downloaded subtitle content length: {}", content.len());
        debug!(
            "First 500 chars of content: {}",
            content.chars().take(500).collect::<String>()
        );

        if content.is_empty() {
            return Err(YdlError::SubtitleParsing {
                message: "Empty subtitle content".to_string(),
            });
        }

        Ok(content)
    }
}

// Additional InnerTube API response structures
#[derive(Debug, Deserialize)]
pub struct PlayabilityStatus {
    pub status: String,
    pub reason: Option<String>,
}
