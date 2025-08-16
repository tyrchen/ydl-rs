use crate::error::{YdlError, YdlResult};
use regex::Regex;
use url::Url;

/// YouTube URL parser for extracting video IDs from various URL formats
pub struct YouTubeParser {
    video_id_regex: Regex,
    youtube_domains: Vec<&'static str>,
}

impl Default for YouTubeParser {
    fn default() -> Self {
        Self::new()
    }
}

impl YouTubeParser {
    /// Create a new YouTube parser
    pub fn new() -> Self {
        // YouTube video ID pattern: 11 characters, alphanumeric plus - and _
        let video_id_regex = Regex::new(r"^[a-zA-Z0-9_-]{11}$").expect("Valid video ID regex");

        let youtube_domains = vec![
            "youtube.com",
            "www.youtube.com",
            "youtu.be",
            "m.youtube.com",
            "youtube-nocookie.com",
            "www.youtube-nocookie.com",
        ];

        Self {
            video_id_regex,
            youtube_domains,
        }
    }

    /// Parse a YouTube URL and extract the video ID
    pub fn parse_url(&self, url_str: &str) -> YdlResult<String> {
        // First, try to parse as URL
        let url = Url::parse(url_str).map_err(|_| YdlError::InvalidUrl {
            url: url_str.to_string(),
        })?;

        // Validate it's a YouTube domain
        self.validate_domain(&url)?;

        // Extract video ID based on URL pattern
        self.extract_video_id(&url)
    }

    /// Validate that the URL is from a YouTube domain
    fn validate_domain(&self, url: &Url) -> YdlResult<()> {
        let domain = url.domain().ok_or_else(|| YdlError::InvalidUrl {
            url: url.to_string(),
        })?;

        if !self.youtube_domains.contains(&domain) {
            return Err(YdlError::InvalidUrl {
                url: url.to_string(),
            });
        }

        Ok(())
    }

    /// Extract video ID from various YouTube URL formats
    fn extract_video_id(&self, url: &Url) -> YdlResult<String> {
        let domain = url.domain().unwrap();

        match domain {
            // youtu.be/VIDEO_ID
            "youtu.be" => {
                let path = url.path().trim_start_matches('/');
                // Remove any additional path components
                let video_id = path.split('/').next().unwrap_or("");
                self.validate_and_return_video_id(video_id, url)
            }
            // youtube.com, www.youtube.com, m.youtube.com, etc.
            _ => {
                // Try different patterns
                if let Ok(id) = self.extract_from_watch_url(url) {
                    return Ok(id);
                }
                if let Ok(id) = self.extract_from_embed_url(url) {
                    return Ok(id);
                }
                if let Ok(id) = self.extract_from_shorts_url(url) {
                    return Ok(id);
                }

                Err(YdlError::InvalidUrl {
                    url: url.to_string(),
                })
            }
        }
    }

    /// Extract video ID from /watch?v=VIDEO_ID URLs
    fn extract_from_watch_url(&self, url: &Url) -> YdlResult<String> {
        if url.path() != "/watch" {
            return Err(YdlError::InvalidUrl {
                url: url.to_string(),
            });
        }

        let video_id = url
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value.to_string())
            .ok_or_else(|| YdlError::InvalidUrl {
                url: url.to_string(),
            })?;

        self.validate_and_return_video_id(&video_id, url)
    }

    /// Extract video ID from /embed/VIDEO_ID URLs
    fn extract_from_embed_url(&self, url: &Url) -> YdlResult<String> {
        let path_segments: Vec<&str> = url
            .path_segments()
            .ok_or_else(|| YdlError::InvalidUrl {
                url: url.to_string(),
            })?
            .collect();

        if path_segments.len() >= 2 && path_segments[0] == "embed" {
            let video_id = path_segments[1];
            return self.validate_and_return_video_id(video_id, url);
        }

        Err(YdlError::InvalidUrl {
            url: url.to_string(),
        })
    }

    /// Extract video ID from /shorts/VIDEO_ID URLs
    fn extract_from_shorts_url(&self, url: &Url) -> YdlResult<String> {
        let path_segments: Vec<&str> = url
            .path_segments()
            .ok_or_else(|| YdlError::InvalidUrl {
                url: url.to_string(),
            })?
            .collect();

        if path_segments.len() >= 2 && path_segments[0] == "shorts" {
            let video_id = path_segments[1];
            return self.validate_and_return_video_id(video_id, url);
        }

        Err(YdlError::InvalidUrl {
            url: url.to_string(),
        })
    }

    /// Validate video ID format and return if valid
    fn validate_and_return_video_id(&self, video_id: &str, _url: &Url) -> YdlResult<String> {
        if self.is_valid_video_id(video_id) {
            Ok(video_id.to_string())
        } else {
            Err(YdlError::InvalidVideoId {
                video_id: video_id.to_string(),
            })
        }
    }

    /// Validate that a video ID matches YouTube's format requirements
    pub fn is_valid_video_id(&self, video_id: &str) -> bool {
        self.video_id_regex.is_match(video_id)
    }

    /// Normalize a URL to standard YouTube format
    pub fn normalize_url(&self, url_str: &str) -> YdlResult<String> {
        let video_id = self.parse_url(url_str)?;
        Ok(format!("https://www.youtube.com/watch?v={}", video_id))
    }

    /// Extract video ID directly from string (if it's already a video ID)
    pub fn extract_video_id_direct(&self, input: &str) -> YdlResult<String> {
        if self.is_valid_video_id(input) {
            Ok(input.to_string())
        } else {
            self.parse_url(input)
        }
    }
}

/// Convenience function to parse a YouTube URL
pub fn parse_youtube_url(url: &str) -> YdlResult<String> {
    YouTubeParser::new().parse_url(url)
}

/// Convenience function to validate a video ID
pub fn is_valid_video_id(video_id: &str) -> bool {
    YouTubeParser::new().is_valid_video_id(video_id)
}

/// Convenience function to normalize a YouTube URL
pub fn normalize_youtube_url(url: &str) -> YdlResult<String> {
    YouTubeParser::new().normalize_url(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> YouTubeParser {
        YouTubeParser::new()
    }

    #[test]
    fn test_parse_standard_watch_url() {
        let parser = parser();

        // Standard watch URLs
        let urls = vec![
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?v=dQw4w9WgXcQ",
            "http://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
        ];

        for url in urls {
            let result = parser.parse_url(url);
            assert!(result.is_ok(), "Failed to parse: {}", url);
            assert_eq!(result.unwrap(), "dQw4w9WgXcQ");
        }
    }

    #[test]
    fn test_parse_short_urls() {
        let parser = parser();

        let urls = vec![
            "https://youtu.be/dQw4w9WgXcQ",
            "http://youtu.be/dQw4w9WgXcQ",
            "youtu.be/dQw4w9WgXcQ",
        ];

        for url in urls {
            let result = parser.parse_url(url);
            if result.is_err() {
                // Handle the case where scheme is missing
                let full_url = format!("https://{}", url);
                let result = parser.parse_url(&full_url);
                assert!(result.is_ok(), "Failed to parse: {}", url);
                assert_eq!(result.unwrap(), "dQw4w9WgXcQ");
            } else {
                assert_eq!(result.unwrap(), "dQw4w9WgXcQ");
            }
        }
    }

    #[test]
    fn test_parse_embed_urls() {
        let parser = parser();

        let urls = vec![
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
        ];

        for url in urls {
            let result = parser.parse_url(url);
            assert!(result.is_ok(), "Failed to parse: {}", url);
            assert_eq!(result.unwrap(), "dQw4w9WgXcQ");
        }
    }

    #[test]
    fn test_parse_shorts_urls() {
        let parser = parser();

        let url = "https://www.youtube.com/shorts/dQw4w9WgXcQ";
        let result = parser.parse_url(url);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "dQw4w9WgXcQ");
    }

    #[test]
    fn test_parse_urls_with_additional_params() {
        let parser = parser();

        let urls = vec![
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=10s",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PLrCZdFsaG",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=10s&list=PLrCZdFsaG",
            "https://youtu.be/dQw4w9WgXcQ?t=10s",
        ];

        for url in urls {
            let result = parser.parse_url(url);
            assert!(result.is_ok(), "Failed to parse: {}", url);
            assert_eq!(result.unwrap(), "dQw4w9WgXcQ");
        }
    }

    #[test]
    fn test_invalid_urls() {
        let parser = parser();

        let invalid_urls = vec![
            "https://www.google.com/watch?v=dQw4w9WgXcQ", // Wrong domain
            "https://www.youtube.com/watch",              // No video ID
            "https://www.youtube.com/watch?list=PLrCZdFsaG", // No v parameter
            "https://www.youtube.com/user/someuser",      // User page
            "not-a-url-at-all",                           // Invalid URL format
            "",                                           // Empty string
        ];

        for url in invalid_urls {
            let result = parser.parse_url(url);
            assert!(result.is_err(), "Should fail to parse: {}", url);
        }
    }

    #[test]
    fn test_invalid_video_ids() {
        let parser = parser();

        let invalid_ids = vec![
            "short",                 // Too short
            "way_too_long_video_id", // Too long
            "invalid-chars!",        // Invalid characters
            "dQw4w9WgXc",            // 10 characters (should be 11)
            "dQw4w9WgXcQQ",          // 12 characters (should be 11)
        ];

        for id in invalid_ids {
            assert!(!parser.is_valid_video_id(id), "Should be invalid: {}", id);
        }
    }

    #[test]
    fn test_valid_video_ids() {
        let parser = parser();

        let valid_ids = vec!["dQw4w9WgXcQ", "aBc_123-XyZ", "0123456789a", "_-_-_-_-_-_"];

        for id in valid_ids {
            assert!(parser.is_valid_video_id(id), "Should be valid: {}", id);
        }
    }

    #[test]
    fn test_normalize_url() {
        let parser = parser();

        let test_cases = vec![
            (
                "https://youtu.be/dQw4w9WgXcQ",
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            ),
            (
                "https://www.youtube.com/embed/dQw4w9WgXcQ",
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            ),
            (
                "https://m.youtube.com/watch?v=dQw4w9WgXcQ&t=10s",
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            ),
        ];

        for (input, expected) in test_cases {
            let result = parser.normalize_url(input);
            assert!(result.is_ok(), "Failed to normalize: {}", input);
            assert_eq!(result.unwrap(), expected);
        }
    }

    #[test]
    fn test_extract_video_id_direct() {
        let parser = parser();

        // Test with direct video ID
        let result = parser.extract_video_id_direct("dQw4w9WgXcQ");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "dQw4w9WgXcQ");

        // Test with URL
        let result = parser.extract_video_id_direct("https://youtu.be/dQw4w9WgXcQ");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "dQw4w9WgXcQ");

        // Test with invalid input
        let result = parser.extract_video_id_direct("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_convenience_functions() {
        // Test parse_youtube_url function
        let result = parse_youtube_url("https://youtu.be/dQw4w9WgXcQ");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "dQw4w9WgXcQ");

        // Test is_valid_video_id function
        assert!(is_valid_video_id("dQw4w9WgXcQ"));
        assert!(!is_valid_video_id("invalid"));

        // Test normalize_youtube_url function
        let result = normalize_youtube_url("https://youtu.be/dQw4w9WgXcQ");
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
    }
}
