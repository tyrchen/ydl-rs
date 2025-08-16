use crate::error::{YdlError, YdlResult};
use crate::types::{ParsedSubtitles, SubtitleEntry, SubtitleType};
use encoding_rs::UTF_8;
use regex::Regex;
use std::time::Duration;
use tracing::{debug, warn};

/// Content processor for parsing and converting subtitle formats
pub struct ContentProcessor {
    /// Regex for parsing SRT timestamps
    srt_time_regex: Regex,
    /// Regex for parsing VTT timestamps
    vtt_time_regex: Regex,
    /// Regex for cleaning HTML tags
    html_tag_regex: Regex,
}

impl Default for ContentProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentProcessor {
    /// Create a new content processor
    pub fn new() -> Self {
        let srt_time_regex =
            Regex::new(r"(\d{2}):(\d{2}):(\d{2}),(\d{3}) --> (\d{2}):(\d{2}):(\d{2}),(\d{3})")
                .expect("Valid SRT time regex");

        let vtt_time_regex =
            Regex::new(r"(\d{2}):(\d{2}):(\d{2})\.(\d{3}) --> (\d{2}):(\d{2}):(\d{2})\.(\d{3})")
                .expect("Valid VTT time regex");

        let html_tag_regex = Regex::new(r"<[^>]*>").expect("Valid HTML tag regex");

        Self {
            srt_time_regex,
            vtt_time_regex,
            html_tag_regex,
        }
    }

    /// Process raw subtitle content and convert to the desired format
    pub fn process_content(
        &self,
        raw_content: &str,
        target_format: SubtitleType,
        language: &str,
        clean_content: bool,
        validate_timing: bool,
    ) -> YdlResult<String> {
        debug!(
            "Processing subtitle content, target format: {:?}",
            target_format
        );

        // First, detect encoding and convert to UTF-8 if needed
        let content = self.ensure_utf8(raw_content)?;

        // Parse the content to determine the source format and extract entries
        let parsed = self.parse_subtitle_content(&content, language)?;

        // Validate timing if requested
        if validate_timing {
            self.validate_timing(&parsed.entries)?;
        }

        // Clean content if requested
        let entries = if clean_content {
            self.clean_subtitle_entries(parsed.entries)
        } else {
            parsed.entries
        };

        // Convert to target format
        self.convert_to_format(&entries, target_format, language)
    }

    /// Ensure content is valid UTF-8
    fn ensure_utf8(&self, content: &str) -> YdlResult<String> {
        // Try to detect encoding if not UTF-8
        let (decoded, _encoding_used, had_errors) = UTF_8.decode(content.as_bytes());

        if had_errors {
            warn!("Encoding errors detected, attempting to fix");
            // Try common encodings for subtitles
            let encodings = [
                encoding_rs::WINDOWS_1252,
                encoding_rs::ISO_8859_2,
                encoding_rs::UTF_16LE,
                encoding_rs::UTF_16BE,
            ];

            for encoding in &encodings {
                let (decoded, _, had_errors) = encoding.decode(content.as_bytes());
                if !had_errors {
                    debug!("Successfully decoded using {:?}", encoding.name());
                    return Ok(decoded.to_string());
                }
            }

            // If all else fails, use the UTF-8 decode with replacement chars
            Ok(decoded.to_string())
        } else {
            Ok(content.to_string())
        }
    }

    /// Parse subtitle content and determine format
    fn parse_subtitle_content(&self, content: &str, language: &str) -> YdlResult<ParsedSubtitles> {
        debug!("Parsing subtitle content, {} bytes", content.len());

        // Try different parsers based on content characteristics
        if content.contains("WEBVTT") {
            self.parse_vtt_content(content, language)
        } else if content.contains("<?xml") || content.contains("<transcript") {
            self.parse_youtube_xml_content(content, language)
        } else if self.srt_time_regex.is_match(content) {
            self.parse_srt_content(content, language)
        } else if content.contains("-->") {
            // Might be VTT without header
            self.parse_vtt_content(content, language)
        } else {
            // Try to parse as plain text with timing info
            self.parse_plain_text_content(content, language)
        }
    }

    /// Parse SRT format content
    fn parse_srt_content(&self, content: &str, language: &str) -> YdlResult<ParsedSubtitles> {
        let mut entries = Vec::new();
        let blocks = content.split("\n\n");

        for block in blocks {
            let block = block.trim();
            if block.is_empty() {
                continue;
            }

            let lines: Vec<&str> = block.lines().collect();
            if lines.len() < 3 {
                continue;
            }

            // Skip sequence number (first line)
            let timing_line = lines[1];
            let text_lines = &lines[2..];

            if let Some(captures) = self.srt_time_regex.captures(timing_line) {
                let start = self.parse_srt_time(&captures, 1)?;
                let end = self.parse_srt_time(&captures, 5)?;
                let text = text_lines.join("\n");

                entries.push(SubtitleEntry::new(start, end, text));
            }
        }

        if entries.is_empty() {
            return Err(YdlError::SubtitleParsing {
                message: "No valid SRT entries found".to_string(),
            });
        }

        Ok(ParsedSubtitles::new(entries, language.to_string()).with_format(SubtitleType::Srt))
    }

    /// Parse VTT format content
    fn parse_vtt_content(&self, content: &str, language: &str) -> YdlResult<ParsedSubtitles> {
        let mut entries = Vec::new();
        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0;

        // Skip WEBVTT header and metadata
        while i < lines.len() {
            let line = lines[i].trim();
            if line.is_empty() || line.starts_with("WEBVTT") || line.starts_with("NOTE") {
                i += 1;
                continue;
            }
            break;
        }

        // Parse cue blocks
        while i < lines.len() {
            let line = lines[i].trim();

            if line.is_empty() {
                i += 1;
                continue;
            }

            // Check if this line contains timing
            if let Some(captures) = self.vtt_time_regex.captures(line) {
                let start = self.parse_vtt_time(&captures, 1)?;
                let end = self.parse_vtt_time(&captures, 5)?;

                // Collect text lines
                i += 1;
                let mut text_lines = Vec::new();
                while i < lines.len() && !lines[i].trim().is_empty() {
                    text_lines.push(lines[i]);
                    i += 1;
                }

                let text = text_lines.join("\n");
                entries.push(SubtitleEntry::new(start, end, text));
            } else {
                // Skip cue identifier line
                i += 1;
            }
        }

        if entries.is_empty() {
            return Err(YdlError::SubtitleParsing {
                message: "No valid VTT entries found".to_string(),
            });
        }

        Ok(ParsedSubtitles::new(entries, language.to_string()).with_format(SubtitleType::Vtt))
    }

    /// Parse YouTube XML transcript format
    fn parse_youtube_xml_content(
        &self,
        content: &str,
        language: &str,
    ) -> YdlResult<ParsedSubtitles> {
        let mut entries = Vec::new();

        // Try the newer srv3 format first (uses <p> tags)
        let p_regex =
            Regex::new(r#"<p\s+t="(\d+)"(?:\s+d="(\d+)")?[^>]*>(.*?)</p>"#).map_err(|e| {
                YdlError::SubtitleParsing {
                    message: format!("Invalid XML regex: {}", e),
                }
            })?;

        let s_regex =
            Regex::new(r#"<s[^>]*>([^<]*)</s>"#).map_err(|e| YdlError::SubtitleParsing {
                message: format!("Invalid s tag regex: {}", e),
            })?;

        for captures in p_regex.captures_iter(content) {
            let start_str = captures.get(1).unwrap().as_str();
            let duration_str = captures.get(2).map(|m| m.as_str()).unwrap_or("1000");
            let inner_content = captures.get(3).unwrap().as_str();

            // Parse start time (in milliseconds for srv3 format)
            let start_ms: u64 = start_str.parse().unwrap_or(0);
            let duration_ms: u64 = duration_str.parse().unwrap_or(1000);

            let start = Duration::from_millis(start_ms);
            let end = Duration::from_millis(start_ms + duration_ms);

            // Extract text from <s> tags or use the inner content directly
            let text = if inner_content.contains("<s") {
                let mut words = Vec::new();
                for s_capture in s_regex.captures_iter(inner_content) {
                    if let Some(word) = s_capture.get(1) {
                        words.push(word.as_str());
                    }
                }
                words.join("")
            } else {
                inner_content.to_string()
            };

            // Decode HTML entities
            let decoded_text = html_escape::decode_html_entities(&text)
                .to_string()
                .trim()
                .to_string();

            // Skip empty entries
            if !decoded_text.is_empty() {
                entries.push(SubtitleEntry::new(start, end, decoded_text));
            }
        }

        // If no <p> tags found, try the older <text> format
        if entries.is_empty() {
            let text_regex =
                Regex::new(r#"<text start="([^"]+)"(?:\s+dur="([^"]+)")?>([^<]*)</text>"#)
                    .map_err(|e| YdlError::SubtitleParsing {
                        message: format!("Invalid XML regex: {}", e),
                    })?;

            for captures in text_regex.captures_iter(content) {
                let start_str = captures.get(1).unwrap().as_str();
                let duration_str = captures.get(2).map(|m| m.as_str()).unwrap_or("1");
                let text = captures.get(3).unwrap().as_str();

                // Parse start time (usually in seconds as float)
                let start_secs: f64 = start_str.parse().unwrap_or(0.0);
                let duration_secs: f64 = duration_str.parse().unwrap_or(1.0);

                let start = Duration::from_secs_f64(start_secs);
                let end = Duration::from_secs_f64(start_secs + duration_secs);

                // Decode HTML entities
                let decoded_text = html_escape::decode_html_entities(text).to_string();

                entries.push(SubtitleEntry::new(start, end, decoded_text));
            }
        }

        if entries.is_empty() {
            return Err(YdlError::SubtitleParsing {
                message: "No valid XML transcript entries found".to_string(),
            });
        }

        Ok(ParsedSubtitles::new(entries, language.to_string()).with_format(SubtitleType::Raw))
    }

    /// Parse plain text with minimal timing information
    fn parse_plain_text_content(
        &self,
        content: &str,
        language: &str,
    ) -> YdlResult<ParsedSubtitles> {
        // For plain text, create artificial timing
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

        if lines.is_empty() {
            return Err(YdlError::SubtitleParsing {
                message: "No content found in plain text".to_string(),
            });
        }

        let mut entries = Vec::new();
        let avg_duration = Duration::from_secs(3); // 3 seconds per line

        for (i, line) in lines.iter().enumerate() {
            let start = Duration::from_secs((i as u64) * 3);
            let end = start + avg_duration;

            entries.push(SubtitleEntry::new(start, end, line.to_string()));
        }

        Ok(ParsedSubtitles::new(entries, language.to_string()).with_format(SubtitleType::Txt))
    }

    /// Parse SRT timestamp from regex captures
    fn parse_srt_time(
        &self,
        captures: &regex::Captures,
        start_group: usize,
    ) -> YdlResult<Duration> {
        let hours: u64 = captures
            .get(start_group)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|_| YdlError::SubtitleParsing {
                message: "Invalid SRT hour format".to_string(),
            })?;
        let minutes: u64 = captures
            .get(start_group + 1)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|_| YdlError::SubtitleParsing {
                message: "Invalid SRT minute format".to_string(),
            })?;
        let seconds: u64 = captures
            .get(start_group + 2)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|_| YdlError::SubtitleParsing {
                message: "Invalid SRT second format".to_string(),
            })?;
        let millis: u64 = captures
            .get(start_group + 3)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|_| YdlError::SubtitleParsing {
                message: "Invalid SRT millisecond format".to_string(),
            })?;

        Ok(Duration::from_millis(
            hours * 3600000 + minutes * 60000 + seconds * 1000 + millis,
        ))
    }

    /// Parse VTT timestamp from regex captures
    fn parse_vtt_time(
        &self,
        captures: &regex::Captures,
        start_group: usize,
    ) -> YdlResult<Duration> {
        let hours: u64 = captures
            .get(start_group)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|_| YdlError::SubtitleParsing {
                message: "Invalid VTT hour format".to_string(),
            })?;
        let minutes: u64 = captures
            .get(start_group + 1)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|_| YdlError::SubtitleParsing {
                message: "Invalid VTT minute format".to_string(),
            })?;
        let seconds: u64 = captures
            .get(start_group + 2)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|_| YdlError::SubtitleParsing {
                message: "Invalid VTT second format".to_string(),
            })?;
        let millis: u64 = captures
            .get(start_group + 3)
            .unwrap()
            .as_str()
            .parse()
            .map_err(|_| YdlError::SubtitleParsing {
                message: "Invalid VTT millisecond format".to_string(),
            })?;

        Ok(Duration::from_millis(
            hours * 3600000 + minutes * 60000 + seconds * 1000 + millis,
        ))
    }

    /// Clean subtitle entries by removing HTML tags and normalizing text
    fn clean_subtitle_entries(&self, entries: Vec<SubtitleEntry>) -> Vec<SubtitleEntry> {
        entries
            .into_iter()
            .map(|mut entry| {
                // Remove HTML tags
                entry.text = self.html_tag_regex.replace_all(&entry.text, "").to_string();

                // Normalize whitespace
                entry.text = entry.text.split_whitespace().collect::<Vec<_>>().join(" ");

                // Remove common subtitle formatting
                entry.text = entry
                    .text
                    .replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&amp;", "&")
                    .replace("&quot;", "\"")
                    .replace("&#39;", "'");

                entry
            })
            .collect()
    }

    /// Validate timing consistency
    fn validate_timing(&self, entries: &[SubtitleEntry]) -> YdlResult<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut prev_end = Duration::from_secs(0);

        for (i, entry) in entries.iter().enumerate() {
            // Check that start < end
            if entry.start >= entry.end {
                return Err(YdlError::SubtitleParsing {
                    message: format!("Invalid timing at entry {}: start >= end", i + 1),
                });
            }

            // Check for reasonable duration (not too short or too long)
            let duration = entry.duration();
            if duration < Duration::from_millis(100) {
                warn!(
                    "Very short subtitle duration at entry {}: {:?}",
                    i + 1,
                    duration
                );
            } else if duration > Duration::from_secs(30) {
                warn!(
                    "Very long subtitle duration at entry {}: {:?}",
                    i + 1,
                    duration
                );
            }

            // Check for overlaps or gaps (warning only)
            if entry.start < prev_end {
                warn!("Overlapping subtitles at entry {}", i + 1);
            }

            prev_end = entry.end;
        }

        Ok(())
    }

    /// Convert subtitle entries to target format
    fn convert_to_format(
        &self,
        entries: &[SubtitleEntry],
        format: SubtitleType,
        language: &str,
    ) -> YdlResult<String> {
        match format {
            SubtitleType::Srt => self.to_srt_format(entries),
            SubtitleType::Vtt => self.to_vtt_format(entries),
            SubtitleType::Txt => self.to_txt_format(entries),
            SubtitleType::Json => self.to_json_format(entries, language),
            SubtitleType::Raw => {
                // For raw format, return as is if we have entries
                if entries.is_empty() {
                    Ok(String::new())
                } else {
                    self.to_srt_format(entries) // Default to SRT for raw
                }
            }
        }
    }

    /// Convert to SRT format
    fn to_srt_format(&self, entries: &[SubtitleEntry]) -> YdlResult<String> {
        let mut result = String::new();

        for (i, entry) in entries.iter().enumerate() {
            result.push_str(&format!("{}\n", i + 1));
            result.push_str(&format!(
                "{} --> {}\n",
                entry.start_as_srt(),
                entry.end_as_srt()
            ));
            result.push_str(&entry.text);
            result.push_str("\n\n");
        }

        Ok(result)
    }

    /// Convert to VTT format
    fn to_vtt_format(&self, entries: &[SubtitleEntry]) -> YdlResult<String> {
        let mut result = String::from("WEBVTT\n\n");

        for entry in entries {
            result.push_str(&format!(
                "{} --> {}\n",
                entry.start_as_vtt(),
                entry.end_as_vtt()
            ));
            result.push_str(&entry.text);
            result.push_str("\n\n");
        }

        Ok(result)
    }

    /// Convert to plain text format
    fn to_txt_format(&self, entries: &[SubtitleEntry]) -> YdlResult<String> {
        let texts: Vec<String> = entries.iter().map(|e| e.text.clone()).collect();
        Ok(texts.join("\n"))
    }

    /// Convert to JSON format
    fn to_json_format(&self, entries: &[SubtitleEntry], language: &str) -> YdlResult<String> {
        let json_entries: Vec<serde_json::Value> = entries
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "start": entry.start.as_secs_f64(),
                    "end": entry.end.as_secs_f64(),
                    "text": entry.text
                })
            })
            .collect();

        let result = serde_json::json!({
            "language": language,
            "entries": json_entries
        });

        serde_json::to_string_pretty(&result).map_err(YdlError::from)
    }
}

// Simple HTML entity decoder (subset of common entities)
mod html_escape {
    pub fn decode_html_entities(text: &str) -> std::borrow::Cow<'_, str> {
        let mut result = text.to_string();

        result = result.replace("&amp;", "&");
        result = result.replace("&lt;", "<");
        result = result.replace("&gt;", ">");
        result = result.replace("&quot;", "\"");
        result = result.replace("&#39;", "'");
        result = result.replace("&#x27;", "'");
        result = result.replace("&apos;", "'");

        std::borrow::Cow::Owned(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_processor() -> ContentProcessor {
        ContentProcessor::new()
    }

    #[test]
    fn test_parse_srt_content() {
        let processor = test_processor();
        let srt_content = r#"1
00:00:01,000 --> 00:00:03,000
Hello, world!

2
00:00:04,000 --> 00:00:06,000
This is a test.
"#;

        let result = processor.parse_srt_content(srt_content, "en");
        assert!(result.is_ok());

        let parsed = result.unwrap();
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].text, "Hello, world!");
        assert_eq!(parsed.entries[1].text, "This is a test.");
    }

    #[test]
    fn test_parse_vtt_content() {
        let processor = test_processor();
        let vtt_content = r#"WEBVTT

00:00:01.000 --> 00:00:03.000
Hello, world!

00:00:04.000 --> 00:00:06.000
This is a test.
"#;

        let result = processor.parse_vtt_content(vtt_content, "en");
        assert!(result.is_ok());

        let parsed = result.unwrap();
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].text, "Hello, world!");
        assert_eq!(parsed.entries[1].text, "This is a test.");
    }

    #[test]
    fn test_convert_to_srt() {
        let processor = test_processor();
        let entries = vec![SubtitleEntry::new(
            Duration::from_secs(1),
            Duration::from_secs(3),
            "Hello, world!".to_string(),
        )];

        let result = processor.to_srt_format(&entries);
        assert!(result.is_ok());

        let srt = result.unwrap();
        assert!(srt.contains("1\n"));
        assert!(srt.contains("00:00:01,000 --> 00:00:03,000"));
        assert!(srt.contains("Hello, world!"));
    }

    #[test]
    fn test_convert_to_vtt() {
        let processor = test_processor();
        let entries = vec![SubtitleEntry::new(
            Duration::from_secs(1),
            Duration::from_secs(3),
            "Hello, world!".to_string(),
        )];

        let result = processor.to_vtt_format(&entries);
        assert!(result.is_ok());

        let vtt = result.unwrap();
        assert!(vtt.starts_with("WEBVTT"));
        assert!(vtt.contains("00:00:01.000 --> 00:00:03.000"));
        assert!(vtt.contains("Hello, world!"));
    }

    #[test]
    fn test_convert_to_txt() {
        let processor = test_processor();
        let entries = vec![
            SubtitleEntry::new(
                Duration::from_secs(1),
                Duration::from_secs(3),
                "Hello, world!".to_string(),
            ),
            SubtitleEntry::new(
                Duration::from_secs(4),
                Duration::from_secs(6),
                "This is a test.".to_string(),
            ),
        ];

        let result = processor.to_txt_format(&entries);
        assert!(result.is_ok());

        let txt = result.unwrap();
        assert_eq!(txt, "Hello, world!\nThis is a test.");
    }

    #[test]
    fn test_clean_subtitle_entries() {
        let processor = test_processor();
        let entries = vec![SubtitleEntry::new(
            Duration::from_secs(1),
            Duration::from_secs(3),
            "<b>Hello</b>, &amp; world!".to_string(),
        )];

        let cleaned = processor.clean_subtitle_entries(entries);
        assert_eq!(cleaned[0].text, "Hello, & world!");
    }

    #[test]
    fn test_validate_timing() {
        let processor = test_processor();

        // Valid timing
        let valid_entries = vec![
            SubtitleEntry::new(
                Duration::from_secs(1),
                Duration::from_secs(3),
                "Test".to_string(),
            ),
            SubtitleEntry::new(
                Duration::from_secs(4),
                Duration::from_secs(6),
                "Test".to_string(),
            ),
        ];
        assert!(processor.validate_timing(&valid_entries).is_ok());

        // Invalid timing (start >= end)
        let invalid_entries = vec![SubtitleEntry::new(
            Duration::from_secs(3),
            Duration::from_secs(1),
            "Test".to_string(),
        )];
        assert!(processor.validate_timing(&invalid_entries).is_err());
    }

    #[test]
    fn test_parse_youtube_xml() {
        let processor = test_processor();
        let xml_content = r#"<?xml version="1.0" encoding="utf-8"?>
<transcript>
<text start="1.5" dur="2.5">Hello world</text>
<text start="4.0" dur="3.0">This is a test</text>
</transcript>"#;

        let result = processor.parse_youtube_xml_content(xml_content, "en");
        assert!(result.is_ok());

        let parsed = result.unwrap();
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].text, "Hello world");
        assert_eq!(parsed.entries[1].text, "This is a test");
    }
}
