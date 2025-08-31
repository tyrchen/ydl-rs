use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
        ChatCompletionRequestSystemMessageContent, ChatCompletionRequestUserMessage,
        ChatCompletionRequestUserMessageContent, CreateChatCompletionRequest,
    },
};
use std::env;
use tracing::{debug, info};
use ydl::{VideoMetadata, YdlError, YdlResult};

pub struct BlogGenerator {
    client: Client<OpenAIConfig>,
}

impl BlogGenerator {
    pub async fn new() -> YdlResult<Self> {
        // Get API key from environment
        let api_key = env::var("OPENAI_API_KEY").map_err(|_| YdlError::Configuration {
            message: "OPENAI_API_KEY environment variable not set".to_string(),
        })?;

        let config = OpenAIConfig::new().with_api_key(api_key);
        let client = Client::with_config(config);

        Ok(Self { client })
    }

    pub async fn generate_blog(
        &self,
        subtitle_content: &str,
        metadata: &VideoMetadata,
        target_language: &str,
    ) -> YdlResult<String> {
        info!("Generating blog for video: {}", metadata.video_id);
        debug!(
            "Target language: {}, subtitle length: {} chars",
            target_language,
            subtitle_content.len()
        );

        let system_prompt = self.build_system_prompt(target_language);
        let user_prompt = self.build_user_prompt(subtitle_content, metadata);

        let request = CreateChatCompletionRequest {
            model: "gpt-5".to_string(), // Using GPT-5 for superior content generation
            messages: vec![
                ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                    content: ChatCompletionRequestSystemMessageContent::Text(system_prompt),
                    name: None,
                }),
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: ChatCompletionRequestUserMessageContent::Text(user_prompt),
                    name: None,
                }),
            ],
            max_completion_tokens: Some(20000),
            ..Default::default()
        };

        let response =
            self.client
                .chat()
                .create(request)
                .await
                .map_err(|e| YdlError::Processing {
                    message: format!("OpenAI API error: {}", e),
                })?;

        let blog_content = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_ref())
            .ok_or_else(|| YdlError::Processing {
                message: "No content received from OpenAI API".to_string(),
            })?;

        info!(
            "Successfully generated blog with {} characters",
            blog_content.len()
        );
        Ok(blog_content.clone())
    }

    fn build_system_prompt(&self, target_language: &str) -> String {
        let blogger_style = include_str!("./blogger.md");

        format!(
            r#"{blogger_style}

You are about to transform raw YouTube video subtitles into a masterful technical blog post. Your mission is to create content that not only informs but transforms how readers think about the subject matter.

LANGUAGE REQUIREMENT: Write the entire blog post in {target_language}.

YOUR APPROACH:

1. **Start with a Compelling Hook**
   - Open with an intriguing question, paradox, or unexpected observation from the video
   - Draw readers in with a narrative that makes them NEED to know more
   - Connect the technical topic to a universal human experience or challenge

2. **Apply the Socratic Method Throughout**
   - Instead of stating "X works like this," ask "What would happen if...?"
   - Guide readers to discoveries: "Notice how this pattern emerges when..."
   - Pose reflective questions: "Why do you think this approach was chosen?"
   - Challenge assumptions: "But what if we turned this problem inside out?"

3. **Build from First Principles**
   - Identify the fundamental truths in the video content
   - Strip away jargon and rebuild concepts from their essence
   - Ask: "What is the simplest truth we can start from?"
   - Show how complex systems emerge from simple rules

4. **Weave Literary Excellence**
   - Use vivid metaphors that illuminate technical concepts
   - Create narrative tension: problems, struggles, breakthroughs
   - Employ analogies that connect code to art, architecture, music, nature
   - Write with rhythm and flow - vary sentence length, create breathing space

5. **Layer Progressive Disclosure**
   - Start with the accessible and gradually unveil complexity
   - Each section should build on the previous, creating "aha!" moments
   - Use concrete examples before abstract concepts
   - Include code snippets that tell a story, not just demonstrate syntax

6. **Explore the "Why" Behind the "What"**
   - Don't just explain the technology - explore why it exists
   - What human problem does it solve?
   - What philosophical questions does it raise?
   - How does it reflect broader patterns in computer science and life?

7. **Include Thought Experiments**
   - "Imagine you're building this from scratch..."
   - "What if we had infinite resources?"
   - "How would this look in 10 years?"
   - Challenge readers to think beyond current limitations

8. **Connect to Broader Themes**
   - Link technical concepts to timeless principles
   - Show how this knowledge applies beyond coding
   - Draw parallels to other fields and disciplines
   - Reveal the universal patterns hiding in specific implementations

9. **End with Transformative Insights**
   - Don't just summarize - synthesize
   - Leave readers with questions that will haunt them (in a good way)
   - Provide a new lens through which to view the topic
   - Inspire action: what can readers do with this knowledge?

REMEMBER: You're not just translating subtitles - you're crafting an intellectual journey. Every paragraph should serve both to inform and to inspire deeper thinking. Your readers should finish not just knowing more, but thinking differently.

Make the complex accessible without dumbing it down. Make the technical philosophical without losing precision. Make the educational entertaining without sacrificing depth.

Now, transform these subtitles into a blog post that readers will bookmark, share, and return to repeatedly."#,
            blogger_style = blogger_style,
            target_language = target_language
        )
    }

    fn build_user_prompt(&self, subtitle_content: &str, metadata: &VideoMetadata) -> String {
        let video_context = if !metadata.title.is_empty() {
            format!("Video Title: {}\n", metadata.title)
        } else {
            String::new()
        };

        let duration_context = if let Some(duration) = metadata.duration {
            format!("Duration: {} minutes\n", duration.as_secs() / 60)
        } else {
            String::new()
        };

        format!(
            r#"Transform these YouTube video subtitles into an exceptional technical blog post:

{video_context}{duration_context}
Video ID: {video_id}
URL: https://www.youtube.com/watch?v={video_id}

RAW SUBTITLE CONTENT:
{subtitle_content}

YOUR MISSION:

First, deeply understand the content:
- What is the core problem or concept being discussed?
- What are the key insights or breakthroughs presented?
- What struggles or challenges are revealed?
- What patterns or principles emerge from the specifics?

Then, craft your blog post following this structure:

1. **THE HOOK** (1-2 paragraphs)
   - Start with a paradox, surprising fact, or profound question from the video
   - Make it impossible for readers to stop reading
   - Connect to a universal experience or curiosity

2. **THE JOURNEY** (Main body - multiple sections)
   - Organize around 3-5 major themes or concepts from the video
   - For each theme:
     * Start with a question that makes readers think
     * Build understanding from first principles
     * Use a metaphor or analogy to illuminate the concept
     * Include a concrete example or thought experiment
     * Connect to broader patterns in technology and life
   - Use progressive disclosure - each section builds on the previous
   - Include relevant code snippets that tell a story (if applicable)

3. **THE TRANSFORMATION** (Conclusion - 2-3 paragraphs)
   - Synthesize the key insights into a new perspective
   - Pose a challenging question that readers will ponder
   - Suggest practical next steps or experiments
   - End with a thought that changes how readers see the topic

CRITICAL REQUIREMENTS:
- Extract the EXACT technical details and examples from the subtitles
- Preserve the speaker's key insights and unique perspectives
- Transform lists and explanations into narrative flow
- Turn verbal explanations into vivid written prose
- Make implicit connections explicit
- Add depth through questions and thought experiments
- Ensure technical accuracy while adding philosophical depth

TONE: Write as if you're having a fascinating late-night conversation with a brilliant friend - informal enough to be engaging, profound enough to be memorable, clear enough to be understood, deep enough to be worth re-reading.

Remember: This blog post should be so good that readers will:
1. Bookmark it for future reference
2. Share it with colleagues
3. Think about it days later
4. Use it to explain concepts to others

Now, begin your transformation..."#,
            video_context = video_context,
            duration_context = duration_context,
            video_id = metadata.video_id,
            subtitle_content = self.truncate_content(subtitle_content, 8000), // Limit content to avoid token limits
        )
    }

    fn truncate_content<'a>(&self, content: &'a str, max_chars: usize) -> &'a str {
        if content.len() <= max_chars {
            content
        } else {
            // Try to truncate at a sentence or paragraph boundary
            let truncated = &content[..max_chars];
            if let Some(last_period) = truncated.rfind('.') {
                if last_period > max_chars * 3 / 4 {
                    // If we found a period in the last quarter, use it
                    &content[..last_period + 1]
                } else {
                    truncated
                }
            } else {
                truncated
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_truncate_content() {
        let config = OpenAIConfig::new();
        let generator = BlogGenerator {
            client: Client::with_config(config), // This won't work without API key, but fine for testing truncation
        };

        let short_content = "This is short.";
        assert_eq!(
            generator.truncate_content(short_content, 100),
            short_content
        );

        let long_content = "This is a very long content that exceeds the limit. It has multiple sentences. This should be truncated properly.";
        let truncated = generator.truncate_content(long_content, 50);
        assert!(truncated.len() <= 50);
        assert!(truncated.ends_with('.') || truncated.len() == 50);
    }

    #[test]
    fn test_build_user_prompt() {
        let config = OpenAIConfig::new();
        let generator = BlogGenerator {
            client: Client::with_config(config), // This won't work without API key, but fine for testing prompt building
        };

        let metadata = VideoMetadata {
            title: "Test Video".to_string(),
            video_id: "test123".to_string(),
            duration: Some(Duration::from_secs(300)),
            available_subtitles: Vec::new(),
        };

        let prompt = generator.build_user_prompt("Test subtitle content", &metadata);

        assert!(prompt.contains("Test Video"));
        assert!(prompt.contains("test123"));
        assert!(prompt.contains("5 minutes"));
        assert!(prompt.contains("Test subtitle content"));
    }

    #[test]
    fn test_build_system_prompt() {
        let config = OpenAIConfig::new();
        let generator = BlogGenerator {
            client: Client::with_config(config), // This won't work without API key, but fine for testing prompt building
        };

        let prompt = generator.build_system_prompt("English");

        assert!(prompt.contains("English"));
        assert!(prompt.contains("Socratic Method"));
        assert!(prompt.contains("First Principles Thinking"));
    }
}
