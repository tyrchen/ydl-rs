use crate::error::{YdlError, YdlResult};
use crate::types::{PlayerResponse, SubtitleTrack, SubtitleTrackType, VideoMetadata, YdlOptions};
use crate::youtube_client::YouTubeSubtitleExtractor;
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info};

/// YouTube subtitle extractor for discovering and downloading subtitles
pub struct SubtitleExtractor {
    client: Client,
    options: YdlOptions,
    youtube_client: YouTubeSubtitleExtractor,
}

impl SubtitleExtractor {
    /// Create a new subtitle extractor
    pub fn new(options: YdlOptions) -> YdlResult<Self> {
        let mut headers = reqwest::header::HeaderMap::new();

        // Set a realistic User-Agent
        let user_agent = options.user_agent.as_deref().unwrap_or(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
        );
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_str(user_agent).map_err(|_| {
                YdlError::Configuration {
                    message: "Invalid user agent".to_string(),
                }
            })?,
        );

        // Set other headers to mimic a real browser
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
            ),
        );
        headers.insert(
            reqwest::header::ACCEPT_LANGUAGE,
            reqwest::header::HeaderValue::from_static("en-US,en;q=0.5"),
        );
        // Remove Accept-Encoding to get uncompressed response
        // reqwest will handle compression automatically if we don't set this

        let mut client_builder = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(options.timeout_seconds))
            .redirect(reqwest::redirect::Policy::limited(10));

        // Add proxy if specified
        if let Some(proxy_url) = &options.proxy {
            let proxy = reqwest::Proxy::all(proxy_url).map_err(|e| YdlError::Configuration {
                message: format!("Invalid proxy URL: {}", e),
            })?;
            client_builder = client_builder.proxy(proxy);
        }

        let client = client_builder
            .build()
            .map_err(|e| YdlError::Configuration {
                message: format!("Failed to create HTTP client: {}", e),
            })?;

        let youtube_client = YouTubeSubtitleExtractor::new()?;

        Ok(Self {
            client,
            options,
            youtube_client,
        })
    }

    /// Discover available subtitle tracks for a video
    pub async fn discover_tracks(&self, video_id: &str) -> YdlResult<Vec<SubtitleTrack>> {
        info!("Discovering subtitle tracks for video: {}", video_id);

        // Try different methods to find subtitles
        let mut tracks = Vec::new();

        // Method 1: Try InnerTube API first (most reliable)
        if let Ok(innertube_tracks) = self.youtube_client.discover_tracks(video_id).await {
            info!("Found {} tracks via InnerTube API", innertube_tracks.len());
            tracks.extend(innertube_tracks);
        }

        // Method 2: Try to get from watch page as fallback
        if tracks.is_empty()
            && let Ok(page_tracks) = self.discover_from_watch_page(video_id).await
        {
            tracks.extend(page_tracks);
        }

        // Method 3: Try mobile endpoint if no tracks found
        if tracks.is_empty()
            && let Ok(mobile_tracks) = self.discover_from_mobile_page(video_id).await
        {
            tracks.extend(mobile_tracks);
        }

        // Method 4: Try direct API approach
        if tracks.is_empty()
            && let Ok(api_tracks) = self.discover_from_api(video_id).await
        {
            tracks.extend(api_tracks);
        }

        // Filter based on options
        self.filter_tracks(tracks, video_id)
    }

    /// Get video metadata including available subtitles
    pub async fn get_video_metadata(&self, video_id: &str) -> YdlResult<VideoMetadata> {
        info!("Getting video metadata for: {}", video_id);

        let url = format!("https://www.youtube.com/watch?v={}", video_id);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(self.map_http_error(response.status(), video_id));
        }

        let html = response.text().await?;

        // Extract basic video info and player response
        let title = self.extract_video_title(&html)?;
        let player_response = self.extract_player_response(&html)?;

        let mut metadata = VideoMetadata::new(video_id.to_string(), title);

        // Extract duration if available
        if let Some(video_details) = &player_response.video_details
            && let Some(length_str) = &video_details.length_seconds
            && let Ok(length) = length_str.parse::<u64>()
        {
            metadata = metadata.with_duration(Duration::from_secs(length));
        }

        // Get available subtitles
        let tracks = self.discover_tracks(video_id).await?;
        metadata = metadata.with_subtitles(tracks);

        Ok(metadata)
    }

    /// Download subtitle content from a track
    pub async fn download_content(
        &self,
        track: &SubtitleTrack,
        video_id: &str,
    ) -> YdlResult<String> {
        // If we have a URL from the track, try to use it
        if let Some(base_url) = &track.url {
            // First try with the InnerTube client (which handles authentication better)
            info!("Downloading subtitle content via InnerTube client");
            match self.youtube_client.download_content(base_url).await {
                Ok(content) if !content.is_empty() => {
                    debug!(
                        "Downloaded {} bytes of subtitle content via InnerTube",
                        content.len()
                    );

                    // Save to file for debugging
                    #[cfg(debug_assertions)]
                    {
                        use std::fs;
                        let _ = fs::write("/tmp/subtitle_content.xml", &content);
                        debug!("Saved subtitle content to /tmp/subtitle_content.xml for debugging");
                    }

                    return Ok(content);
                }
                Err(e) => {
                    debug!("InnerTube download failed: {}, trying direct download", e);
                }
                _ => {}
            }

            // Fallback to direct download
            // Add format parameter - srv3 is YouTube's XML format that works well
            let url = if base_url.contains("fmt=") {
                base_url.clone()
            } else {
                let separator = if base_url.contains('?') { "&" } else { "?" };
                format!("{}{separator}fmt=srv3", base_url)
            };

            info!("Trying direct download from: {}", url);
            let response = self.client.get(&url).send().await?;

            if response.status().is_success() {
                let content = response.text().await?;
                if !content.is_empty() {
                    debug!("Downloaded {} bytes of subtitle content", content.len());
                    return Ok(content);
                }
            }
        }

        // Fallback: construct a simple subtitle URL
        // This works for many videos that have auto-generated subtitles
        let fallback_url = format!(
            "https://www.youtube.com/api/timedtext?v={}&lang={}&fmt=srv3",
            video_id, track.language_code
        );

        info!("Trying fallback subtitle URL: {}", fallback_url);
        let response = self.client.get(&fallback_url).send().await?;

        if !response.status().is_success() {
            return Err(YdlError::SubtitleDiscoveryError {
                message: format!("HTTP {}: Failed to download subtitles", response.status()),
            });
        }

        let content = response.text().await?;
        debug!("Downloaded {} bytes of subtitle content", content.len());

        if content.is_empty() {
            return Err(YdlError::SubtitleParsing {
                message: "Empty subtitle content received".to_string(),
            });
        }

        debug!(
            "Subtitle content preview (first 500 chars): {}",
            &content.chars().take(500).collect::<String>()
        );

        Ok(content)
    }

    /// Discover subtitles from the main watch page
    async fn discover_from_watch_page(&self, video_id: &str) -> YdlResult<Vec<SubtitleTrack>> {
        debug!("Trying to discover subtitles from watch page");

        let url = format!("https://www.youtube.com/watch?v={}", video_id);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(self.map_http_error(response.status(), video_id));
        }

        let html = response.text().await?;

        // Debug: save HTML to file for inspection
        #[cfg(debug_assertions)]
        {
            use std::fs;
            let _ = fs::write("/tmp/youtube_watch_page.html", &html);
            debug!("Saved HTML to /tmp/youtube_watch_page.html for debugging");
        }

        let player_response = self.extract_player_response(&html)?;

        // Extract tracks but construct simpler URLs that work
        let mut tracks = Vec::new();
        if let Some(captions) = &player_response.captions
            && let Some(tracklist) = &captions.player_captions_tracklist_renderer
            && let Some(caption_tracks) = &tracklist.caption_tracks
        {
            for track in caption_tracks {
                // Instead of using the base_url from player response (which needs auth),
                // construct a simple URL that often works for public videos
                let simple_url = format!(
                    "https://www.youtube.com/api/timedtext?v={}&lang={}",
                    video_id, track.language_code
                );

                let language_name = track
                    .name
                    .as_ref()
                    .and_then(|n| {
                        n.simple_text.as_deref().or_else(|| {
                            n.runs
                                .as_ref()
                                .and_then(|runs| runs.first().map(|r| r.text.as_str()))
                        })
                    })
                    .unwrap_or(&track.language_code);

                let track_type = if track.kind == Some("asr".to_string()) {
                    SubtitleTrackType::AutoGenerated
                } else {
                    SubtitleTrackType::Manual
                };

                let subtitle_track = SubtitleTrack::new(
                    track.language_code.clone(),
                    language_name.to_string(),
                    track_type,
                )
                .with_url(simple_url)
                .with_translatable(track.is_translatable.unwrap_or(false));

                tracks.push(subtitle_track);
            }
        }

        if tracks.is_empty() {
            // Fallback to the original method if our simple approach doesn't work
            self.extract_tracks_from_player_response(&player_response, video_id)
        } else {
            Ok(tracks)
        }
    }

    /// Discover subtitles from mobile endpoint
    async fn discover_from_mobile_page(&self, video_id: &str) -> YdlResult<Vec<SubtitleTrack>> {
        debug!("Trying to discover subtitles from mobile page");

        let url = format!("https://m.youtube.com/watch?v={}", video_id);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(self.map_http_error(response.status(), video_id));
        }

        let html = response.text().await?;
        let player_response = self.extract_player_response(&html)?;

        self.extract_tracks_from_player_response(&player_response, video_id)
    }

    /// Discover subtitles using direct API approach
    async fn discover_from_api(&self, video_id: &str) -> YdlResult<Vec<SubtitleTrack>> {
        debug!("Trying to discover subtitles from API");

        // Try the get_video_info endpoint
        let url = format!(
            "https://www.youtube.com/get_video_info?video_id={}&el=detailpage&ps=default&eurl=&gl=US&hl=en",
            video_id
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(YdlError::SubtitleDiscoveryError {
                message: "Failed to fetch video info".to_string(),
            });
        }

        let content = response.text().await?;

        // Parse URL-encoded response
        let params: HashMap<String, String> = url::form_urlencoded::parse(content.as_bytes())
            .into_owned()
            .collect();

        if let Some(player_response_str) = params.get("player_response")
            && let Ok(player_response) = serde_json::from_str::<PlayerResponse>(player_response_str)
        {
            return self.extract_tracks_from_player_response(&player_response, video_id);
        }

        Err(YdlError::SubtitleDiscoveryError {
            message: "No player response found in API response".to_string(),
        })
    }

    /// Extract player response JSON from HTML
    fn extract_player_response(&self, html: &str) -> YdlResult<PlayerResponse> {
        debug!(
            "Attempting to extract player response from HTML (length: {})",
            html.len()
        );

        // Look for ytInitialPlayerResponse (with or without var)
        let patterns = [
            "var ytInitialPlayerResponse = ",
            "ytInitialPlayerResponse = ",
        ];
        for pattern in &patterns {
            debug!("Searching for pattern: {}", pattern);
            if let Some(start) = html.find(pattern) {
                debug!("Found pattern at position {}", start);
                let json_start = start + pattern.len();
                // Look for the end of the JSON object - it should end with };
                if let Some(json_end) = html[json_start..].find("};") {
                    // Include the closing brace but not the semicolon
                    let json_str = &html[json_start..json_start + json_end + 1];
                    debug!("Found ytInitialPlayerResponse, attempting to parse");
                    match serde_json::from_str::<PlayerResponse>(json_str) {
                        Ok(player_response) => {
                            debug!("Successfully parsed player response");
                            if let Some(_captions) = &player_response.captions {
                                debug!("Player response has captions field");
                            } else {
                                debug!("Player response has NO captions field");
                            }
                            return Ok(player_response);
                        }
                        Err(e) => {
                            debug!("Failed to parse player response: {}", e);
                        }
                    }
                }
            }
        }

        // Alternative pattern
        if let Some(start) = html.find("\"PLAYER_RESPONSE\":\"") {
            let json_start = start + "\"PLAYER_RESPONSE\":\"".len();
            if let Some(json_end) = html[json_start..].find("\",\"") {
                let escaped_json = &html[json_start..json_start + json_end];
                // Unescape the JSON
                let unescaped = escaped_json.replace("\\\"", "\"").replace("\\\\", "\\");
                if let Ok(player_response) = serde_json::from_str::<PlayerResponse>(&unescaped) {
                    return Ok(player_response);
                }
            }
        }

        Err(YdlError::MetadataParsingError {
            message: "Could not find player response in HTML".to_string(),
        })
    }

    /// Extract video title from HTML
    fn extract_video_title(&self, html: &str) -> YdlResult<String> {
        // Try to find title in various places
        if let Some(start) = html.find("<title>")
            && let Some(end) = html[start..].find("</title>")
        {
            let title = &html[start + 7..start + end];
            // Remove " - YouTube" suffix if present
            let clean_title = title.replace(" - YouTube", "");
            return Ok(clean_title);
        }

        // Fallback: try to find in JSON
        if let Some(start) = html.find("\"title\":\"") {
            let title_start = start + 9;
            if let Some(title_end) = html[title_start..].find("\"") {
                let title = &html[title_start..title_start + title_end];
                return Ok(title.to_string());
            }
        }

        Ok("Unknown Title".to_string())
    }

    /// Extract subtitle tracks from player response
    fn extract_tracks_from_player_response(
        &self,
        player_response: &PlayerResponse,
        video_id: &str,
    ) -> YdlResult<Vec<SubtitleTrack>> {
        let mut tracks = Vec::new();

        debug!("Extracting tracks from player response");
        if let Some(captions) = &player_response.captions {
            debug!("Found captions in player response");
            if let Some(tracklist) = &captions.player_captions_tracklist_renderer {
                debug!("Found tracklist renderer");
                if let Some(caption_tracks) = &tracklist.caption_tracks {
                    debug!("Found {} caption tracks", caption_tracks.len());
                    for track in caption_tracks {
                        let language_name = track
                            .name
                            .as_ref()
                            .and_then(|n| {
                                n.simple_text.as_deref().or_else(|| {
                                    n.runs
                                        .as_ref()
                                        .and_then(|runs| runs.first().map(|r| r.text.as_str()))
                                })
                            })
                            .unwrap_or(&track.language_code)
                            .to_string();

                        // Determine track type based on kind or vss_id
                        let track_type = if track.kind.as_deref() == Some("asr") {
                            SubtitleTrackType::AutoGenerated
                        } else {
                            SubtitleTrackType::Manual
                        };

                        debug!(
                            "Found subtitle track: lang={}, name={}, type={:?}, has_url={}",
                            track.language_code,
                            &language_name,
                            &track_type,
                            !track.base_url.is_empty()
                        );

                        let subtitle_track = SubtitleTrack::new(
                            track.language_code.clone(),
                            language_name,
                            track_type,
                        )
                        .with_url(track.base_url.clone())
                        .with_translatable(track.is_translatable.unwrap_or(false));

                        tracks.push(subtitle_track);
                    }
                }
            }
        }

        if tracks.is_empty() {
            Err(YdlError::NoSubtitlesAvailable {
                video_id: video_id.to_string(),
            })
        } else {
            Ok(tracks)
        }
    }

    /// Filter tracks based on options
    fn filter_tracks(
        &self,
        tracks: Vec<SubtitleTrack>,
        video_id: &str,
    ) -> YdlResult<Vec<SubtitleTrack>> {
        if tracks.is_empty() {
            return Err(YdlError::NoSubtitlesAvailable {
                video_id: video_id.to_string(),
            });
        }

        let mut filtered = tracks;

        // Filter by language preference
        if let Some(preferred_lang) = &self.options.language {
            let lang_matches: Vec<_> = filtered
                .iter()
                .filter(|track| track.language_code == *preferred_lang)
                .cloned()
                .collect();

            if !lang_matches.is_empty() {
                filtered = lang_matches;
            }
        }

        // Filter by track type preferences
        if !self.options.allow_auto_generated {
            filtered.retain(|track| track.track_type != SubtitleTrackType::AutoGenerated);
        }

        // Prefer manual subtitles if requested
        if self.options.prefer_manual {
            let manual_tracks: Vec<_> = filtered
                .iter()
                .filter(|track| track.track_type == SubtitleTrackType::Manual)
                .cloned()
                .collect();

            if !manual_tracks.is_empty() {
                filtered = manual_tracks;
            }
        }

        if filtered.is_empty() {
            // Check if we filtered out everything due to preferences
            if !self.options.allow_auto_generated {
                return Err(YdlError::OnlyAutoGenerated {
                    video_id: video_id.to_string(),
                });
            }
            return Err(YdlError::NoSubtitlesAvailable {
                video_id: video_id.to_string(),
            });
        }

        Ok(filtered)
    }

    /// Map HTTP status codes to appropriate errors
    fn map_http_error(&self, status: reqwest::StatusCode, video_id: &str) -> YdlError {
        match status.as_u16() {
            404 => YdlError::VideoNotFound {
                video_id: video_id.to_string(),
            },
            403 => YdlError::VideoRestricted {
                video_id: video_id.to_string(),
            },
            429 => YdlError::RateLimited { retry_after: 60 },
            503 => YdlError::ServiceUnavailable,
            _ => YdlError::SubtitleDiscoveryError {
                message: format!("HTTP {} error", status),
            },
        }
    }

    /// Select the best subtitle track based on preferences
    pub fn select_best_track<'a>(
        &'a self,
        tracks: &'a [SubtitleTrack],
    ) -> Option<&'a SubtitleTrack> {
        if tracks.is_empty() {
            return None;
        }

        // If language is specified, prefer that, but also consider manual preference
        if let Some(preferred_lang) = &self.options.language {
            // First try to find a manual track in the preferred language
            if self.options.prefer_manual
                && let Some(track) = tracks.iter().find(|t| {
                    t.language_code == *preferred_lang && t.track_type == SubtitleTrackType::Manual
                })
            {
                return Some(track);
            }

            // Then try any track in the preferred language
            if let Some(track) = tracks.iter().find(|t| t.language_code == *preferred_lang) {
                return Some(track);
            }
        }

        // Prefer manual over auto-generated (for any language)
        if self.options.prefer_manual
            && let Some(manual) = tracks
                .iter()
                .find(|t| t.track_type == SubtitleTrackType::Manual)
        {
            return Some(manual);
        }

        // Fall back to first available track
        tracks.first()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_options() -> YdlOptions {
        YdlOptions::new().timeout(10)
    }

    #[tokio::test]
    async fn test_extractor_creation() {
        let options = test_options();
        let extractor = SubtitleExtractor::new(options);
        assert!(extractor.is_ok());
    }

    #[test]
    fn test_extract_video_title() {
        let extractor = SubtitleExtractor::new(test_options()).unwrap();

        let html = r"
        <html>
        <head>
            <title>Test Video - YouTube</title>
        </head>
        <body></body>
        </html>
        ";

        let title = extractor.extract_video_title(html);
        assert!(title.is_ok());
        assert_eq!(title.unwrap(), "Test Video");
    }

    #[test]
    fn test_filter_tracks() {
        let extractor = SubtitleExtractor::new(test_options()).unwrap();

        let tracks = vec![
            SubtitleTrack::new(
                "en".to_string(),
                "English".to_string(),
                SubtitleTrackType::Manual,
            ),
            SubtitleTrack::new(
                "en".to_string(),
                "English (auto)".to_string(),
                SubtitleTrackType::AutoGenerated,
            ),
            SubtitleTrack::new(
                "es".to_string(),
                "Spanish".to_string(),
                SubtitleTrackType::Manual,
            ),
        ];

        let filtered = extractor.filter_tracks(tracks, "test_video_id");
        assert!(filtered.is_ok());

        let result = filtered.unwrap();
        assert!(!result.is_empty());
        // Should prefer manual tracks by default
        assert!(
            result
                .iter()
                .any(|t| t.track_type == SubtitleTrackType::Manual)
        );
    }

    #[test]
    fn test_select_best_track() {
        let options = YdlOptions::new().language("en").prefer_manual(true);
        let extractor = SubtitleExtractor::new(options).unwrap();

        let tracks = vec![
            SubtitleTrack::new(
                "es".to_string(),
                "Spanish".to_string(),
                SubtitleTrackType::Manual,
            ),
            SubtitleTrack::new(
                "en".to_string(),
                "English (auto)".to_string(),
                SubtitleTrackType::AutoGenerated,
            ),
            SubtitleTrack::new(
                "en".to_string(),
                "English".to_string(),
                SubtitleTrackType::Manual,
            ),
        ];

        let best = extractor.select_best_track(&tracks);
        assert!(best.is_some());

        let selected = best.unwrap();
        assert_eq!(selected.language_code, "en");
        assert_eq!(selected.track_type, SubtitleTrackType::Manual);
    }

    #[test]
    fn test_map_http_error() {
        let extractor = SubtitleExtractor::new(test_options()).unwrap();

        let error_404 = extractor.map_http_error(reqwest::StatusCode::NOT_FOUND, "test123");
        assert!(matches!(error_404, YdlError::VideoNotFound { .. }));

        let error_403 = extractor.map_http_error(reqwest::StatusCode::FORBIDDEN, "test123");
        assert!(matches!(error_403, YdlError::VideoRestricted { .. }));

        let error_429 = extractor.map_http_error(reqwest::StatusCode::TOO_MANY_REQUESTS, "test123");
        assert!(matches!(error_429, YdlError::RateLimited { .. }));
    }
}
