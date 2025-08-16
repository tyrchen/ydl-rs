use ydl::{SubtitleType, Ydl, YdlOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // YouTube video URL - use a video that is known to have subtitles
    let url = "https://www.youtube.com/watch?v=sSpULGNHyoI";

    println!("Testing YouTube subtitle downloader");
    println!("URL: {}", url);
    println!();

    // Create options
    let options = YdlOptions::new()
        .language("en") // Request English subtitles
        .allow_auto_generated(true) // Allow auto-generated subtitles
        .prefer_manual(true) // But prefer manual ones if available
        .timeout(30) // 30 second timeout
        .clean_content(true) // Clean HTML tags from content
        .validate_timing(true); // Validate subtitle timing

    // Create the downloader
    println!("Creating downloader...");
    let downloader = Ydl::new(url, options)?;

    println!("Video ID: {}", downloader.video_id());
    println!("Normalized URL: {}", downloader.normalized_url());
    println!();

    // List available subtitle tracks
    println!("Discovering available subtitle tracks...");
    match downloader.available_subtitles().await {
        Ok(tracks) => {
            if tracks.is_empty() {
                println!("No subtitle tracks found for this video.");
            } else {
                println!("Found {} subtitle track(s):", tracks.len());
                for track in &tracks {
                    println!(
                        "  - {} ({}) - {:?}",
                        track.language_name, track.language_code, track.track_type
                    );
                }
                println!();
            }
        }
        Err(e) => {
            eprintln!("Error discovering subtitles: {}", e);
        }
    }

    // Get video metadata
    println!("Getting video metadata...");
    match downloader.metadata().await {
        Ok(metadata) => {
            println!("Title: {}", metadata.title);
            if let Some(duration) = metadata.duration {
                let total_secs = duration.as_secs();
                let hours = total_secs / 3600;
                let minutes = (total_secs % 3600) / 60;
                let seconds = total_secs % 60;
                println!("Duration: {:02}:{:02}:{:02}", hours, minutes, seconds);
            }
            println!();
        }
        Err(e) => {
            eprintln!("Error getting metadata: {}", e);
        }
    }

    // Try to download subtitles in SRT format
    println!("Attempting to download subtitles in SRT format...");
    match downloader.subtitle_with_retry(SubtitleType::Srt).await {
        Ok(content) => {
            println!(
                "Successfully downloaded {} characters of SRT content",
                content.len()
            );

            // Show first few lines as preview
            let lines: Vec<&str> = content.lines().take(10).collect();
            println!("\nFirst few lines of subtitle content:");
            println!("=====================================");
            for line in lines {
                println!("{}", line);
            }
            println!("=====================================");

            // Save to file
            let filename = format!("{}.srt", downloader.video_id());
            std::fs::write(&filename, content)?;
            println!("\nSubtitles saved to: {}", filename);
        }
        Err(e) => {
            eprintln!("Error downloading subtitles: {}", e);
            eprintln!("This might happen if:");
            eprintln!("  - The video doesn't have subtitles");
            eprintln!("  - The video is private or restricted");
            eprintln!("  - There's a network issue");
        }
    }

    Ok(())
}
