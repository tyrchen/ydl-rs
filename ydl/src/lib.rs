pub mod error;
pub mod extractor;
pub mod parser;
pub mod processor;
pub mod types;
pub mod youtube_client;

pub use error::{YdlError, YdlResult};
pub use types::{
    ParsedSubtitles, SubtitleEntry, SubtitleResult, SubtitleTrack, SubtitleTrackType, SubtitleType,
    VideoMetadata, YdlOptions,
};

use extractor::SubtitleExtractor;
use parser::YouTubeParser;
use processor::ContentProcessor;
use std::sync::Arc;
use tracing::{debug, error, info};

/// Main orchestrator for subtitle downloads
pub struct Ydl {
    url: String,
    video_id: String,
    options: YdlOptions,
    extractor: Arc<SubtitleExtractor>,
    processor: ContentProcessor,
}

impl Ydl {
    /// Create a new downloader instance for a specific URL
    pub fn new(url: &str, options: YdlOptions) -> YdlResult<Self> {
        info!("Initializing Ydl for URL: {}", url);

        let parser = YouTubeParser::new();
        let video_id = parser.parse_url(url)?;

        debug!("Extracted video ID: {}", video_id);

        let extractor = Arc::new(SubtitleExtractor::new(options.clone())?);
        let processor = ContentProcessor::new();

        Ok(Self {
            url: url.to_string(),
            video_id,
            options,
            extractor,
            processor,
        })
    }

    /// Download subtitles in the specified format
    pub async fn subtitle(&self, subtitle_type: SubtitleType) -> YdlResult<String> {
        info!("Downloading subtitle in format: {:?}", subtitle_type);

        // Discover available subtitle tracks
        let tracks = self.extractor.discover_tracks(&self.video_id).await?;

        if tracks.is_empty() {
            return Err(YdlError::NoSubtitlesAvailable {
                video_id: self.video_id.clone(),
            });
        }

        // Select the best track based on options
        let selected_track = self.extractor.select_best_track(&tracks).ok_or_else(|| {
            YdlError::NoSubtitlesAvailable {
                video_id: self.video_id.clone(),
            }
        })?;

        debug!(
            "Selected track: {} ({})",
            selected_track.language_name, selected_track.track_type
        );

        // Download the subtitle content
        let raw_content = self
            .extractor
            .download_content(selected_track, &self.video_id)
            .await?;

        // Process and convert the content
        let processed_content = self.processor.process_content(
            &raw_content,
            subtitle_type,
            &selected_track.language_code,
            self.options.clean_content,
            self.options.validate_timing,
        )?;

        Ok(processed_content)
    }

    /// Download subtitles in the specified format (async variant)
    pub async fn subtitle_async(&self, subtitle_type: SubtitleType) -> YdlResult<String> {
        self.subtitle(subtitle_type).await
    }

    /// List all available subtitle tracks for the video
    pub async fn available_subtitles(&self) -> YdlResult<Vec<SubtitleTrack>> {
        info!("Discovering available subtitle tracks");
        self.extractor.discover_tracks(&self.video_id).await
    }

    /// Download multiple subtitle formats at once
    pub async fn subtitles(&self, types: &[SubtitleType]) -> YdlResult<Vec<SubtitleResult>> {
        info!("Downloading multiple subtitle formats: {:?}", types);

        // Discover tracks once
        let tracks = self.extractor.discover_tracks(&self.video_id).await?;

        if tracks.is_empty() {
            return Err(YdlError::NoSubtitlesAvailable {
                video_id: self.video_id.clone(),
            });
        }

        let selected_track = self.extractor.select_best_track(&tracks).ok_or_else(|| {
            YdlError::NoSubtitlesAvailable {
                video_id: self.video_id.clone(),
            }
        })?;

        // Download content once
        let raw_content = self
            .extractor
            .download_content(selected_track, &self.video_id)
            .await?;

        // Process for each requested format
        let mut results = Vec::new();

        for &subtitle_type in types {
            match self.processor.process_content(
                &raw_content,
                subtitle_type,
                &selected_track.language_code,
                self.options.clean_content,
                self.options.validate_timing,
            ) {
                Ok(content) => {
                    results.push(SubtitleResult::new(
                        content,
                        subtitle_type,
                        selected_track.language_code.clone(),
                        selected_track.track_type.clone(),
                    ));
                }
                Err(e) => {
                    error!("Failed to process format {:?}: {}", subtitle_type, e);
                    return Err(e);
                }
            }
        }

        Ok(results)
    }

    /// Get video metadata without downloading subtitles
    pub async fn metadata(&self) -> YdlResult<VideoMetadata> {
        info!("Getting video metadata");
        self.extractor.get_video_metadata(&self.video_id).await
    }

    /// Get the video ID for this instance
    pub fn video_id(&self) -> &str {
        &self.video_id
    }

    /// Get the original URL for this instance
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Get the normalized YouTube URL
    pub fn normalized_url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.video_id)
    }

    /// Check if subtitles are likely available (quick check)
    pub async fn has_subtitles(&self) -> bool {
        match self.extractor.discover_tracks(&self.video_id).await {
            Ok(tracks) => !tracks.is_empty(),
            Err(_) => false,
        }
    }

    /// Download subtitle with retry logic
    pub async fn subtitle_with_retry(&self, subtitle_type: SubtitleType) -> YdlResult<String> {
        let mut retries = 0;
        let max_retries = self.options.max_retries;

        loop {
            match self.subtitle(subtitle_type).await {
                Ok(content) => return Ok(content),
                Err(e) => {
                    if retries >= max_retries {
                        return Err(e);
                    }

                    if e.is_retryable() {
                        retries += 1;
                        let delay = e.retry_delay().unwrap_or(1);

                        debug!(
                            "Retrying in {}s (attempt {} of {})",
                            delay, retries, max_retries
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Future extension for method chaining
    pub fn with_language(mut self, lang: &str) -> Self {
        self.options.language = Some(lang.to_string());
        self
    }

    /// Future extension for format preference
    pub fn with_auto_generated(mut self, allow: bool) -> Self {
        self.options.allow_auto_generated = allow;
        self
    }
}

// Convenience functions for one-off operations

/// Quick function to download a subtitle
pub async fn download_subtitle(url: &str, format: SubtitleType) -> YdlResult<String> {
    let downloader = Ydl::new(url, YdlOptions::default())?;
    downloader.subtitle(format).await
}

/// Quick function to list available subtitles
pub async fn list_subtitles(url: &str) -> YdlResult<Vec<SubtitleTrack>> {
    let downloader = Ydl::new(url, YdlOptions::default())?;
    downloader.available_subtitles().await
}

/// Quick function to get video metadata
pub async fn get_metadata(url: &str) -> YdlResult<VideoMetadata> {
    let downloader = Ydl::new(url, YdlOptions::default())?;
    downloader.metadata().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ydl_creation() {
        let options = YdlOptions::default();
        let result = Ydl::new("https://www.youtube.com/watch?v=dQw4w9WgXcQ", options);
        assert!(result.is_ok());

        let ydl = result.unwrap();
        assert_eq!(ydl.video_id(), "dQw4w9WgXcQ");
        assert_eq!(ydl.url(), "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    }

    #[test]
    fn test_ydl_invalid_url() {
        let options = YdlOptions::default();
        let result = Ydl::new("https://www.google.com/", options);
        assert!(result.is_err());
    }

    #[test]
    fn test_ydl_fluent_interface() {
        let options = YdlOptions::default();
        let ydl = Ydl::new("https://www.youtube.com/watch?v=dQw4w9WgXcQ", options)
            .unwrap()
            .with_language("en")
            .with_auto_generated(false);

        assert_eq!(ydl.options.language, Some("en".to_string()));
        assert!(!ydl.options.allow_auto_generated);
    }

    #[test]
    fn test_normalized_url() {
        let options = YdlOptions::default();
        let ydl = Ydl::new("https://youtu.be/dQw4w9WgXcQ", options).unwrap();
        assert_eq!(
            ydl.normalized_url(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
    }

    // Note: Network tests would require actual YouTube URLs and network access
    // In a real implementation, these would be integration tests with mock servers
}
