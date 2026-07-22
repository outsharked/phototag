use std::path::Path;

use anyhow::{bail, Context, Result};
use phototag_common::TagResponse;

pub struct TaggerClient {
    http: reqwest::Client,
    server_url: String,
}

impl TaggerClient {
    pub fn new(server_url: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .expect("building reqwest client");
        Self { http, server_url }
    }

    pub async fn tag_image(&self, path: &Path) -> Result<Vec<String>> {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("reading {}", path.display()))?;
        let content_type = content_type_for(path);
        let url = format!("{}/tag", self.server_url.trim_end_matches('/'));

        let response = self
            .http
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(bytes)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("phototag-server returned {status}: {body}");
        }

        let parsed: TagResponse = response
            .json()
            .await
            .context("parsing phototag-server response")?;
        Ok(parsed.keywords)
    }
}

fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "tiff" => "image/tiff",
        "heic" => "image/heic",
        _ => "application/octet-stream",
    }
}
