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

    pub async fn extract_keywords(
        &self,
        image_bytes: &[u8],
        content_type: &str,
    ) -> Result<Vec<String>> {
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
        // DEFAULT_PROMPT asks for "3 to 8 concise keywords", so a genuine
        // answer never yields just one. A lone survivor is far more likely
        // to be a refusal/hedge fragment ("I'm sorry", "Unfortunately")
        // that happened to be short enough to slip past the plausibility
        // filter than an actual one-keyword answer, so treat it the same
        // as zero: a failed parse.
        if keywords.len() < 2 {
            bail!("no keywords could be parsed from LLM response: {content:?}");
        }
        Ok(keywords)
    }
}

/// Best-effort heuristic parsing of free-text LLM output — not
/// adversarially robust. A short trailing conversational sign-off (e.g.
/// "Hope this helps!") can occasionally slip through `is_plausible_keyword`
/// as an extra spurious keyword alongside genuine ones, since a short
/// closing clause is structurally indistinguishable from a legitimate
/// short keyword. This is a known, accepted limitation: the damage is
/// bounded (one low-value extra word among otherwise-correct keywords,
/// not systematic corruption), and chasing every possible short
/// sign-off/preamble phrasing with more string heuristics is an unbounded
/// fight. If this becomes a real problem in practice, the better fix is
/// requesting structured/JSON output from the gateway rather than adding
/// more free-text parsing rules.
fn parse_keywords(raw: &str) -> Vec<String> {
    let unfenced = strip_code_fence(raw.trim());
    let unprefaced = strip_preamble(&unfenced);
    let trimmed = unprefaced.trim();

    // The model sometimes answers with a JSON array despite the prompt
    // asking for a plain comma-separated list; try that first.
    if let Ok(list) = serde_json::from_str::<Vec<String>>(trimmed) {
        return clean_keywords(list);
    }

    let cleaned = trimmed.trim_start_matches('[').trim_end_matches(']');

    let words: Vec<String> = if cleaned.contains(',') {
        // A comma-separated candidate can still carry a trailing chatty
        // sign-off glued on by a newline (e.g. "sunset\n\nLet me know if
        // you need anything else!"). Split each comma-delimited piece on
        // newlines too, so the real keyword and the leftover prose become
        // separate candidates instead of one fused, unusable string —
        // `is_plausible_keyword` then drops the prose fragment on its own
        // merits rather than taking the keyword down with it.
        cleaned
            .split(',')
            .flat_map(|piece| piece.lines())
            .map(|s| s.to_string())
            .collect()
    } else if cleaned.contains('\n') {
        cleaned.lines().map(strip_list_marker).collect()
    } else {
        vec![cleaned.to_string()]
    };

    clean_keywords(words)
}

/// Strips a ```` ``` ```` / ```` ```json ```` code fence, keeping only the
/// content between the fence markers (and discarding any surrounding prose)
/// if one is present.
fn strip_code_fence(text: &str) -> String {
    let Some(start) = text.find("```") else {
        return text.to_string();
    };
    let after_open = &text[start + 3..];

    // Skip an optional language tag (e.g. "json") right after the opening
    // fence, up to the end of that line.
    let after_lang = match after_open.find('\n') {
        Some(nl)
            if after_open[..nl]
                .trim()
                .chars()
                .all(|c| c.is_ascii_alphanumeric()) =>
        {
            &after_open[nl + 1..]
        }
        _ => after_open,
    };

    match after_lang.find("```") {
        Some(end) => after_lang[..end].trim().to_string(),
        None => after_lang.trim().to_string(),
    }
}

/// Strips a conversational lead-in like "Here are the keywords:" by taking
/// only the text after the last colon on the first line, when one is
/// present. Leaves colon-free text (the common case) untouched.
fn strip_preamble(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or("");
    match first_line.rfind(':') {
        Some(colon_idx) => text[colon_idx + 1..].trim_start().to_string(),
        None => text.to_string(),
    }
}

/// Strips a leading list marker such as `1.`, `2)`, `-`, `*`, or `•` from a
/// single line of a newline-separated list.
fn strip_list_marker(line: &str) -> String {
    let trimmed = line.trim().trim_start_matches(['-', '*', '•']).trim();

    let digits_end = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    if digits_end > 0 {
        if let Some(rest) = trimmed[digits_end..]
            .strip_prefix('.')
            .or_else(|| trimmed[digits_end..].strip_prefix(')'))
        {
            return rest.trim().to_string();
        }
    }

    trimmed.to_string()
}

/// A real keyword is never more than this many characters — beyond this,
/// a candidate is a leftover prose fragment (preamble/sign-off), not a
/// word or short phrase.
const MAX_KEYWORD_LEN: usize = 40;

/// A real keyword is a word or a short phrase ("golden retriever"), never
/// a run of many words strung into a sentence.
const MAX_KEYWORD_WORDS: usize = 5;

fn clean_keywords(list: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    list.into_iter()
        .map(|k| normalize_keyword(&k))
        .filter(|k| !k.is_empty())
        .filter(|k| is_plausible_keyword(k))
        .filter(|k| seen.insert(k.to_lowercase()))
        .collect()
}

/// Trims whitespace, surrounding quote characters, and trailing sentence
/// punctuation (`.`, `!`, `?`) from a single candidate keyword.
fn normalize_keyword(raw: &str) -> String {
    raw.trim()
        .trim_matches(|c: char| c == '"' || c == '\'')
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .trim()
        .to_string()
}

/// A structural backstop against leftover prose fragments (conversational
/// preambles/sign-offs the string-pattern heuristics above didn't catch,
/// or any other phrasing we haven't anticipated): a real keyword is a word
/// or short phrase, never multi-line and never sentence-length. Candidates
/// that fail this get dropped rather than corrupting the result — the
/// caller's `keywords.is_empty()` check still catches the case where
/// nothing plausible survives.
fn is_plausible_keyword(candidate: &str) -> bool {
    !candidate.contains(['\n', '\r'])
        && candidate.chars().count() <= MAX_KEYWORD_LEN
        && candidate.split_whitespace().count() <= MAX_KEYWORD_WORDS
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

    #[test]
    fn parse_keywords_strips_markdown_json_fence() {
        assert_eq!(
            parse_keywords("```json\n[\"dog\", \"beach\", \"sunset\"]\n```"),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_strips_plain_code_fence() {
        assert_eq!(
            parse_keywords("```\ndog, beach, sunset\n```"),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_handles_newline_separated_list() {
        assert_eq!(
            parse_keywords("dog\nbeach\nsunset"),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_handles_numbered_list() {
        assert_eq!(
            parse_keywords("1. dog\n2. beach\n3. sunset"),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_handles_bulleted_list() {
        assert_eq!(
            parse_keywords("- dog\n- beach\n- sunset"),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_strips_conversational_preamble() {
        assert_eq!(
            parse_keywords("Here are the keywords: dog, beach, sunset"),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_strips_conversational_preamble_before_newline_list() {
        assert_eq!(
            parse_keywords("Here are the keywords:\ndog\nbeach\nsunset"),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_strips_trailing_sentence_punctuation() {
        assert_eq!(
            parse_keywords("dog, beach, sunset."),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_drops_trailing_conversational_sign_off() {
        assert_eq!(
            parse_keywords(
                "Keywords: dog, beach, sunset\n\nLet me know if you need anything else!"
            ),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_drops_overlong_candidate() {
        assert_eq!(
            parse_keywords(
                "dog, beach, this is a very long sentence fragment that is not a real keyword at all"
            ),
            vec!["dog", "beach"]
        );
    }

    // The plausibility filter alone can't distinguish a short refusal/hedge
    // opener from a genuinely short keyword — both survive `parse_keywords`
    // as a single candidate here. It's `GatewayClient::extract_keywords`'s
    // `keywords.len() < 2` check (tied to DEFAULT_PROMPT's "3 to 8 keywords"
    // contract) that turns this single leftover fragment into a bailed
    // error rather than a bogus result; see the
    // `errors_on_apology_refusal_response` / `errors_on_unfortunately_refusal_response`
    // integration tests in `tests/llm_client.rs`.
    #[test]
    fn parse_keywords_reduces_apology_refusal_to_single_fragment() {
        assert_eq!(
            parse_keywords("I'm sorry, but I cannot help with that request as it violates policy."),
            vec!["I'm sorry"]
        );
    }

    #[test]
    fn parse_keywords_reduces_unfortunately_refusal_to_single_fragment() {
        assert_eq!(
            parse_keywords(
                "Unfortunately, I am not able to process this image due to content policy restrictions."
            ),
            vec!["Unfortunately"]
        );
    }
}
