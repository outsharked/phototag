use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "phototag-server")]
pub struct ServerConfig {
    /// Address to bind the HTTP server to.
    #[arg(long, env = "PHOTOTAG_LISTEN_ADDR", default_value = "0.0.0.0:8080")]
    pub listen_addr: String,

    /// Base URL of an OpenAI-compatible chat-completions gateway, e.g.
    /// `http://gateway:8080/v1` — `/chat/completions` is appended.
    #[arg(long, env = "PHOTOTAG_GATEWAY_URL")]
    pub gateway_url: String,

    /// Model name to send in the chat-completion request.
    #[arg(long, env = "PHOTOTAG_GATEWAY_MODEL")]
    pub gateway_model: String,

    /// Request timeout, in seconds. Generous by default since the gateway
    /// may need to wake a sleeping GPU host before it can respond.
    #[arg(long, env = "PHOTOTAG_GATEWAY_TIMEOUT_SECS", default_value_t = 120)]
    pub gateway_timeout_secs: u64,

    /// Overrides the built-in keyword-extraction prompt.
    #[arg(long, env = "PHOTOTAG_PROMPT")]
    pub prompt: Option<String>,
}
