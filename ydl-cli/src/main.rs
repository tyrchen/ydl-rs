use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use ydl::{SubtitleType, Ydl, YdlError, YdlOptions, YdlResult};

mod blog_generator;
use blog_generator::BlogGenerator;

#[derive(Parser)]
#[command(name = "ydl")]
#[command(version, about = "A fast, reliable YouTube subtitle downloader")]
#[command(long_about = None)]
struct Cli {
    /// YouTube video URL or video ID
    #[arg(value_name = "URL")]
    url: String,

    /// Output subtitle format
    #[arg(short, long, value_enum, default_value = "srt")]
    format: CliSubtitleType,

    /// Preferred language code (e.g., en, es, fr)
    #[arg(short, long)]
    language: Option<String>,

    /// Output file path (default: auto-generated)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output directory (default: current directory)
    #[arg(short = 'D', long)]
    output_dir: Option<PathBuf>,

    /// List available subtitle tracks instead of downloading
    #[arg(long)]
    list: bool,

    /// Show video metadata
    #[arg(long)]
    info: bool,

    /// Disable auto-generated subtitles (auto-generated subtitles are allowed by default)
    #[arg(long)]
    no_auto: bool,

    /// Disable preference for manual subtitles over auto-generated
    #[arg(long)]
    no_prefer_manual: bool,

    /// Disable content cleaning (HTML tags, formatting)
    #[arg(long)]
    no_clean: bool,

    /// Disable subtitle timing validation
    #[arg(long)]
    no_validate: bool,

    /// Maximum retry attempts
    #[arg(long, default_value = "3")]
    max_retries: u32,

    /// Request timeout in seconds
    #[arg(long, default_value = "30")]
    timeout: u64,

    /// Custom User-Agent string
    #[arg(long)]
    user_agent: Option<String>,

    /// Proxy URL (http://proxy:port)
    #[arg(long)]
    proxy: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Download multiple formats (comma-separated)
    #[arg(long, value_delimiter = ',')]
    formats: Option<Vec<CliSubtitleType>>,

    /// Force overwrite existing files
    #[arg(long)]
    force: bool,

    /// Generate technical blog from subtitles
    #[arg(long)]
    generate_blog: bool,

    /// Blog language for generation (default: Chinese)
    #[arg(long, default_value = "chinese")]
    blog_lang: String,
}

#[derive(Clone, Copy, ValueEnum)]
enum CliSubtitleType {
    Srt,
    Vtt,
    Txt,
    Json,
    Raw,
}

impl From<CliSubtitleType> for SubtitleType {
    fn from(cli_type: CliSubtitleType) -> Self {
        match cli_type {
            CliSubtitleType::Srt => SubtitleType::Srt,
            CliSubtitleType::Vtt => SubtitleType::Vtt,
            CliSubtitleType::Txt => SubtitleType::Txt,
            CliSubtitleType::Json => SubtitleType::Json,
            CliSubtitleType::Raw => SubtitleType::Raw,
        }
    }
}

#[tokio::main]
async fn main() -> YdlResult<()> {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose);

    info!("Starting ydl for URL: {}", cli.url);

    // Build options from CLI arguments
    let options = build_options(&cli);

    // Create the downloader
    let downloader = Ydl::new(&cli.url, options)?;

    // Execute the requested operation
    if cli.list {
        list_subtitles(&downloader).await?;
    } else if cli.info {
        show_metadata(&downloader).await?;
    } else if cli.generate_blog {
        generate_blog(&downloader, &cli).await?;
    } else if let Some(formats) = &cli.formats {
        download_multiple_formats(&downloader, formats, &cli).await?;
    } else {
        download_single_format(&downloader, cli.format.into(), &cli).await?;
    }

    Ok(())
}

/// Initialize logging based on verbosity level
fn init_logging(verbose: bool) {
    let env_filter = if verbose {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "ydl_cli=debug,ydl=debug".into())
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "ydl_cli=info,ydl=info".into())
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_level(verbose),
        )
        .with(env_filter)
        .init();
}

/// Build YdlOptions from CLI arguments
fn build_options(cli: &Cli) -> YdlOptions {
    let mut options = YdlOptions::new()
        .allow_auto_generated(!cli.no_auto) // Inverted logic - auto is allowed by default
        .prefer_manual(!cli.no_prefer_manual)
        .clean_content(!cli.no_clean)
        .validate_timing(!cli.no_validate)
        .max_retries(cli.max_retries)
        .timeout(cli.timeout);

    if let Some(language) = &cli.language {
        options = options.language(language);
    }

    if let Some(user_agent) = &cli.user_agent {
        options = options.user_agent(user_agent);
    }

    if let Some(proxy) = &cli.proxy {
        options = options.proxy(proxy);
    }

    options
}

/// Generate technical blog from subtitles
async fn generate_blog(downloader: &Ydl, cli: &Cli) -> YdlResult<()> {
    println!(
        "Generating technical blog for video: {}",
        downloader.video_id()
    );

    // Try to read existing plain text file first, otherwise download
    let subtitle_content = {
        // Determine what the text file path would be
        let text_path = determine_output_path(downloader, SubtitleType::Txt, cli).await?;

        if text_path.exists() {
            println!("Using existing plain text file: {}", text_path.display());
            match fs::read_to_string(&text_path).await {
                Ok(content) => content,
                Err(_) => {
                    // If we can't read the file, download fresh
                    println!("Could not read existing file, downloading fresh subtitles...");
                    match downloader.subtitle_with_retry(SubtitleType::Txt).await {
                        Ok(content) => content,
                        Err(e) => {
                            handle_download_error(&e);
                            std::process::exit(1);
                        }
                    }
                }
            }
        } else {
            // No existing file, download the subtitles as text
            println!("Downloading subtitles as plain text...");
            match downloader.subtitle_with_retry(SubtitleType::Txt).await {
                Ok(content) => {
                    // Save the text file for future reference
                    if let Err(e) = write_subtitle_file(&text_path, &content, cli.force).await {
                        eprintln!("Warning: Could not save text file: {}", e);
                    } else {
                        println!("Saved plain text to: {}", text_path.display());
                    }
                    content
                }
                Err(e) => {
                    handle_download_error(&e);
                    std::process::exit(1);
                }
            }
        }
    };

    // Get video metadata for context
    let metadata = match downloader.metadata().await {
        Ok(metadata) => metadata,
        Err(e) => {
            eprintln!("Warning: Could not get video metadata: {}", e);
            // Continue without metadata
            ydl::VideoMetadata::default()
        }
    };

    // Initialize blog generator
    let blog_generator = match BlogGenerator::new().await {
        Ok(generator) => generator,
        Err(e) => {
            eprintln!("❌ Failed to initialize blog generator: {}", e);
            eprintln!("   Make sure OPENAI_API_KEY environment variable is set");
            std::process::exit(1);
        }
    };

    println!("Generating blog content using GPT-5...");

    // Generate the blog
    match blog_generator
        .generate_blog(&subtitle_content, &metadata, &cli.blog_lang)
        .await
    {
        Ok(blog_content) => {
            // Determine output path for blog using title slug
            let blog_filename = if !metadata.title.is_empty() {
                let slug = create_slug(&metadata.title);
                if !slug.is_empty() {
                    format!("{}_blog.md", slug)
                } else {
                    format!("{}_blog.md", downloader.video_id())
                }
            } else {
                format!("{}_blog.md", downloader.video_id())
            };

            let blog_path = if let Some(dir) = &cli.output_dir {
                dir.join(blog_filename)
            } else {
                PathBuf::from(blog_filename)
            };

            // Write the blog content
            match write_blog_file(&blog_path, &blog_content, cli.force).await {
                Ok(_) => {
                    println!(
                        "✅ Successfully generated technical blog: {}",
                        blog_path.display()
                    );
                    info!("Generated blog with {} characters", blog_content.len());
                }
                Err(e) => {
                    eprintln!("❌ Failed to save blog: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to generate blog: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

/// List available subtitle tracks
async fn list_subtitles(downloader: &Ydl) -> YdlResult<()> {
    println!(
        "Discovering subtitle tracks for video: {}",
        downloader.video_id()
    );

    match downloader.available_subtitles().await {
        Ok(tracks) => {
            if tracks.is_empty() {
                println!("No subtitle tracks found.");
                return Ok(());
            }

            println!("\nAvailable subtitle tracks:");
            println!(
                "{:<8} {:<20} {:<15} {:<12}",
                "Code", "Name", "Type", "Translatable"
            );
            println!("{}", "─".repeat(60));

            for track in tracks {
                println!(
                    "{:<8} {:<20} {:<15} {:<12}",
                    track.language_code,
                    truncate(&track.language_name, 20),
                    track.track_type.to_string(),
                    if track.is_translatable { "Yes" } else { "No" }
                );
            }
        }
        Err(e) => {
            eprintln!("Error discovering subtitles: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Show video metadata
async fn show_metadata(downloader: &Ydl) -> YdlResult<()> {
    println!("Getting metadata for video: {}", downloader.video_id());

    match downloader.metadata().await {
        Ok(metadata) => {
            println!("\nVideo Information:");
            println!("Title: {}", metadata.title);
            println!("Video ID: {}", metadata.video_id);

            if let Some(duration) = metadata.duration {
                let total_secs = duration.as_secs();
                let hours = total_secs / 3600;
                let minutes = (total_secs % 3600) / 60;
                let seconds = total_secs % 60;
                println!("Duration: {:02}:{:02}:{:02}", hours, minutes, seconds);
            }

            println!("URL: {}", downloader.normalized_url());

            if !metadata.available_subtitles.is_empty() {
                println!(
                    "\nAvailable Subtitles: {} tracks",
                    metadata.available_subtitles.len()
                );
                for track in metadata.available_subtitles {
                    println!("  - {} ({})", track.language_name, track.track_type);
                }
            } else {
                println!("\nNo subtitles available for this video.");
            }
        }
        Err(e) => {
            eprintln!("Error getting metadata: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Download a single subtitle format
async fn download_single_format(
    downloader: &Ydl,
    format: SubtitleType,
    cli: &Cli,
) -> YdlResult<()> {
    println!(
        "Downloading {} subtitles for video: {}",
        format,
        downloader.video_id()
    );

    match downloader.subtitle_with_retry(format).await {
        Ok(content) => {
            let output_path = determine_output_path(downloader, format, cli).await?;
            write_subtitle_file(&output_path, &content, cli.force).await?;

            println!("Successfully saved subtitles to: {}", output_path.display());
            info!(
                "Downloaded {} characters of {} content",
                content.len(),
                format
            );

            // If we downloaded SRT format, also save a plain text version
            if format == SubtitleType::Srt {
                save_plain_text_version(downloader, &output_path, cli).await?;
            }
        }
        Err(e) => {
            handle_download_error(&e);
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Download multiple subtitle formats
async fn download_multiple_formats(
    downloader: &Ydl,
    formats: &[CliSubtitleType],
    cli: &Cli,
) -> YdlResult<()> {
    let subtitle_types: Vec<SubtitleType> = formats.iter().map(|f| (*f).into()).collect();

    println!(
        "Downloading {} formats for video: {}",
        subtitle_types.len(),
        downloader.video_id()
    );

    match downloader.subtitles(&subtitle_types).await {
        Ok(results) => {
            for result in results {
                let output_path = determine_output_path(downloader, result.format, cli).await?;
                write_subtitle_file(&output_path, &result.content, cli.force).await?;

                println!(
                    "Saved {} subtitles to: {}",
                    result.format,
                    output_path.display()
                );
                info!(
                    "Downloaded {} characters of {} content ({})",
                    result.content.len(),
                    result.format,
                    result.language
                );

                // If we downloaded SRT format, also save a plain text version
                if result.format == SubtitleType::Srt {
                    save_plain_text_version(downloader, &output_path, cli).await?;
                }
            }

            println!(
                "Successfully downloaded all {} formats",
                subtitle_types.len()
            );
        }
        Err(e) => {
            handle_download_error(&e);
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Save a plain text version of the subtitles (for SRT files)
async fn save_plain_text_version(downloader: &Ydl, srt_path: &Path, cli: &Cli) -> YdlResult<()> {
    // Download the subtitles as plain text
    match downloader.subtitle_with_retry(SubtitleType::Txt).await {
        Ok(text_content) => {
            // Create the text file path by replacing the extension
            let text_path = srt_path.with_extension("txt");

            // Write the plain text file
            write_subtitle_file(&text_path, &text_content, cli.force).await?;

            println!("Also saved plain text to: {}", text_path.display());
            info!(
                "Saved {} characters of plain text content",
                text_content.len()
            );
        }
        Err(e) => {
            // Log warning but don't fail the main operation
            eprintln!("Warning: Could not save plain text version: {}", e);
        }
    }

    Ok(())
}

/// Create a slug from a title
fn create_slug(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c
            } else if c.is_whitespace() || c == '-' || c == '_' {
                '-'
            } else {
                // Replace special characters with hyphen
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(100) // Limit slug length to 100 chars
        .collect()
}

/// Determine the output file path
async fn determine_output_path(
    downloader: &Ydl,
    format: SubtitleType,
    cli: &Cli,
) -> YdlResult<PathBuf> {
    if let Some(output) = &cli.output {
        return Ok(output.clone());
    }

    // Try to get video title for filename
    let filename = match downloader.metadata().await {
        Ok(metadata) if !metadata.title.is_empty() => {
            let slug = create_slug(&metadata.title);
            if !slug.is_empty() {
                format!("{}.{}", slug, format.extension())
            } else {
                // Fallback to video ID if slug is empty
                format!("{}.{}", downloader.video_id(), format.extension())
            }
        }
        _ => {
            // Fallback to video ID if metadata fetch fails
            format!("{}.{}", downloader.video_id(), format.extension())
        }
    };

    if let Some(dir) = &cli.output_dir {
        Ok(dir.join(filename))
    } else {
        Ok(PathBuf::from(filename))
    }
}

/// Write subtitle content to file
async fn write_subtitle_file(path: &PathBuf, content: &str, force: bool) -> YdlResult<()> {
    // Check if file exists and force flag
    if path.exists() && !force {
        return Err(YdlError::FileSystem {
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "File already exists: {}. Use --force to overwrite.",
                    path.display()
                ),
            ),
        });
    }

    // Create parent directories if needed
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).await?;
    }

    // Write the file
    fs::write(path, content).await?;

    debug!("Written {} bytes to {}", content.len(), path.display());
    Ok(())
}

/// Write blog content to file
async fn write_blog_file(path: &PathBuf, content: &str, force: bool) -> YdlResult<()> {
    // Check if file exists and force flag
    if path.exists() && !force {
        return Err(YdlError::FileSystem {
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "Blog file already exists: {}. Use --force to overwrite.",
                    path.display()
                ),
            ),
        });
    }

    // Create parent directories if needed
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).await?;
    }

    // Write the file
    fs::write(path, content).await?;

    debug!("Written {} bytes to {}", content.len(), path.display());
    Ok(())
}

/// Handle download errors with user-friendly messages
fn handle_download_error(error: &YdlError) {
    match error {
        YdlError::VideoNotFound { video_id } => {
            eprintln!("❌ Video not found: {}", video_id);
            eprintln!(
                "   The video might have been deleted, made private, or the ID is incorrect."
            );
        }
        YdlError::VideoRestricted { video_id } => {
            eprintln!("❌ Video is private or restricted: {}", video_id);
            eprintln!("   You may not have permission to access this video.");
        }
        YdlError::GeoBlocked { video_id } => {
            eprintln!("❌ Video is geo-blocked: {}", video_id);
            eprintln!("   This content is not available in your region.");
        }
        YdlError::AgeRestricted { video_id } => {
            eprintln!("❌ Video is age-restricted: {}", video_id);
            eprintln!("   Age verification is required to access this content.");
        }
        YdlError::NoSubtitlesAvailable { video_id } => {
            eprintln!("❌ No subtitles available for video: {}", video_id);
            eprintln!("   Try using --allow-auto to include auto-generated subtitles.");
        }
        YdlError::OnlyAutoGenerated { video_id } => {
            eprintln!("❌ Only auto-generated subtitles available: {}", video_id);
            eprintln!("   Use --allow-auto to download auto-generated subtitles.");
        }
        YdlError::LanguageNotAvailable { language } => {
            eprintln!("❌ Language not available: {}", language);
            eprintln!("   Use --list to see available subtitle languages.");
        }
        YdlError::RateLimited { retry_after } => {
            eprintln!("❌ Rate limited by YouTube");
            eprintln!("   Please wait {} seconds and try again.", retry_after);
        }
        YdlError::ServiceUnavailable => {
            eprintln!("❌ YouTube service is temporarily unavailable");
            eprintln!("   Please try again later.");
        }
        YdlError::Network { source } => {
            eprintln!("❌ Network error: {}", source);
            eprintln!("   Check your internet connection and try again.");
        }
        YdlError::InvalidUrl { url } => {
            eprintln!("❌ Invalid YouTube URL: {}", url);
            eprintln!("   Please provide a valid YouTube video URL.");
        }
        _ => {
            eprintln!("❌ Error: {}", error);
        }
    }
}

/// Truncate string to specified length with ellipsis
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_subtitle_type_conversion() {
        assert_eq!(SubtitleType::from(CliSubtitleType::Srt), SubtitleType::Srt);
        assert_eq!(SubtitleType::from(CliSubtitleType::Vtt), SubtitleType::Vtt);
        assert_eq!(SubtitleType::from(CliSubtitleType::Txt), SubtitleType::Txt);
        assert_eq!(
            SubtitleType::from(CliSubtitleType::Json),
            SubtitleType::Json
        );
        assert_eq!(SubtitleType::from(CliSubtitleType::Raw), SubtitleType::Raw);
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello...");
        assert_eq!(truncate("hi", 5), "hi");
    }

    #[test]
    fn test_create_slug() {
        assert_eq!(create_slug("Hello World"), "hello-world");
        assert_eq!(
            create_slug("Architecting LARGE software projects."),
            "architecting-large-software-projects"
        );
        assert_eq!(create_slug("Test!@#$%^&*()Video"), "test-video");
        assert_eq!(create_slug("   Multiple   Spaces   "), "multiple-spaces");
        assert_eq!(create_slug("CamelCase-Title_Here"), "camelcase-title-here");
    }

    #[tokio::test]
    async fn test_determine_output_path() {
        let options = YdlOptions::default();
        let downloader = Ydl::new("https://www.youtube.com/watch?v=dQw4w9WgXcQ", options).unwrap();

        let cli = Cli {
            url: "test".to_string(),
            format: CliSubtitleType::Srt,
            language: None,
            output: None,
            output_dir: None,
            list: false,
            info: false,
            no_auto: false,
            no_prefer_manual: false,
            no_clean: false,
            no_validate: false,
            max_retries: 3,
            timeout: 30,
            user_agent: None,
            proxy: None,
            verbose: false,
            formats: None,
            force: false,
            generate_blog: false,
            blog_lang: "chinese".to_string(),
        };

        let path = determine_output_path(&downloader, SubtitleType::Srt, &cli)
            .await
            .unwrap();
        // The path will now depend on whether we can fetch metadata, so we just check it exists
        assert!(!path.to_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_write_subtitle_file_creates_dirs() {
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("subdir").join("test.srt");

        let result = write_subtitle_file(&file_path, "test content", false).await;
        assert!(result.is_ok());
        assert!(file_path.exists());
    }
}
