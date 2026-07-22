use anyhow::{bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::config::ServerConfig;

pub const DEFAULT_PROMPT: &str = "List 3 to 8 concise keywords describing the objects, \
scene, and setting visible in this image. Respond with ONLY a comma-separated list of \
lowercase keywords and nothing else — no numbering, no sentences, no explanation.";

#[derive(Clone)]
pub struct GatewayClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
    prompt: String,
}

impl GatewayClient {
    pub fn new(config: &ServerConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.gateway_timeout_secs))
            .build()
            .expect("building reqwest client");
        Self {
            http,
            base_url: config.gateway_url.trim_end_matches('/').to_string(),
            model: config.gateway_model.clone(),
            prompt: config
                .prompt
                .clone()
                .unwrap_or_else(|| DEFAULT_PROMPT.to_string()),
        }
    }

    pub async fn extract_keywords(&self, image_bytes: &[u8], content_type: &str) -> Result<Vec<String>> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(image_bytes);
        let data_url = format!("data:{content_type};base64,{b64}");

        let request = ChatRequest {
            model: &self.model,
            messages: vec![ChatMessage {
                role: "user",
                content: vec![
                    ContentPart::Text {
                        text: self.prompt.clone(),
                    },
                    ContentPart::ImageUrl {
                        image_url: ImageUrl { url: data_url },
                    },
                ],
            }],
        };

        let response = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(&request)
            .send()
            .await
            .context("sending request to LLM gateway")?
            .error_for_status()
            .context("LLM gateway returned an error status")?;

        let parsed: ChatCompletionResponse = response
            .json()
            .await
            .context("parsing LLM gateway response as JSON")?;

        let content = parsed
            .choices
            .into_iter()
            .next()
            .context("LLM gateway response had no choices")?
            .message
            .content;

        let keywords = parse_keywords(&content);
        if keywords.is_empty() {
            bail!("no keywords could be parsed from LLM response: {content:?}");
        }
        Ok(keywords)
    }
}

fn parse_keywords(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();

    // The model sometimes answers with a JSON array despite the prompt
    // asking for a plain comma-separated list; try that first.
    if let Ok(list) = serde_json::from_str::<Vec<String>>(trimmed) {
        return clean_keywords(list);
    }

    let cleaned = trimmed.trim_start_matches('[').trim_end_matches(']');
    let words = cleaned
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .collect();
    clean_keywords(words)
}

fn clean_keywords(list: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    list.into_iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .filter(|k| seen.insert(k.to_lowercase()))
        .collect()
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: Vec<ContentPart>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Serialize)]
struct ImageUrl {
    url: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keywords_handles_comma_separated_text() {
        assert_eq!(
            parse_keywords("dog, beach, sunset"),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_handles_json_array() {
        assert_eq!(
            parse_keywords(r#"["dog", "beach", "sunset"]"#),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_dedupes_case_insensitively() {
        assert_eq!(parse_keywords("Dog, dog, DOG"), vec!["Dog"]);
    }

    #[test]
    fn parse_keywords_drops_empty_entries() {
        assert_eq!(parse_keywords("dog, , beach,"), vec!["dog", "beach"]);
    }
}
