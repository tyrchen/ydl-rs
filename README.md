![](https://github.com/tyrchen/ydl-rs/workflows/build/badge.svg)

# ydl-rs

[中文版](README_zh.md) | English

A fast, reliable YouTube subtitle downloader library and CLI tool written in Rust. This project provides both a library (`ydl`) for programmatic access and a command-line interface (`ydl-cli`) for downloading YouTube subtitles in various formats.

## Features

- 🚀 Fast and efficient subtitle downloading
- 📝 Multiple subtitle formats support (SRT, VTT, TXT, JSON, Raw XML)
- 🌐 Language selection for subtitles
- 📊 Video metadata extraction
- 🎯 Automatic subtitle track selection
- 🔄 Retry logic with exponential backoff for reliability
- 📖 Blog post generation from video transcripts (using OpenAI)
- 🛠️ Both library and CLI interfaces

## Installation

### As a Library

Add `ydl` to your `Cargo.toml`:

```toml
[dependencies]
ydl = "0.1.0"
```

### As a CLI Tool

Install the CLI tool using cargo:

```bash
cargo install ydl-cli
```

## Usage

### CLI Usage

#### Download subtitles

```bash
# Download subtitles in SRT format (default)
ydl https://www.youtube.com/watch?v=VIDEO_ID

# Download subtitles in VTT format
ydl https://www.youtube.com/watch?v=VIDEO_ID --format vtt

# Download subtitles in a specific language
ydl https://www.youtube.com/watch?v=VIDEO_ID --language en

# Save to a specific file
ydl https://www.youtube.com/watch?v=VIDEO_ID --output my_subtitles.srt

# Save to a specific directory
ydl https://www.youtube.com/watch?v=VIDEO_ID --output-dir ./subtitles/
```

#### Other operations

```bash
# List available subtitle tracks
ydl https://www.youtube.com/watch?v=VIDEO_ID --list

# Show video metadata
ydl https://www.youtube.com/watch?v=VIDEO_ID --info

# Generate a blog post from video transcript (requires OpenAI API key)
ydl https://www.youtube.com/watch?v=VIDEO_ID --generate-blog

# Enable verbose logging
ydl https://www.youtube.com/watch?v=VIDEO_ID -v
```

### Library Usage

```rust
use ydl::{Ydl, YdlOptions, SubtitleType};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create options
    let options = YdlOptions::builder()
        .language("en")
        .subtitle_type(SubtitleType::Srt)
        .build();

    // Create downloader
    let downloader = Ydl::new("https://www.youtube.com/watch?v=VIDEO_ID", options)?;

    // Download subtitles
    let subtitles = downloader.download().await?;

    // Process subtitles
    println!("Downloaded {} subtitle entries", subtitles.entries.len());
    for entry in &subtitles.entries {
        println!("{} --> {}: {}", entry.start, entry.end, entry.text);
    }

    // Save to file
    downloader.download_to_file("output.srt").await?;

    Ok(())
}
```

## Supported Formats

- **SRT** - SubRip subtitle format
- **VTT** - WebVTT format
- **TXT** - Plain text format
- **JSON** - Structured JSON format
- **Raw** - Original XML format from YouTube

## Environment Variables

- `OPENAI_API_KEY` - Required for blog generation feature

## Project Structure

- `ydl/` - Core library for YouTube subtitle downloading
- `ydl-cli/` - Command-line interface
- `examples/` - Example usage code

## License

This project is distributed under the terms of MIT.

See [LICENSE](LICENSE.md) for details.

Copyright 2025 Tyr Chen
