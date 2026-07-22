use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server_url: String,
    #[serde(default)]
    pub roots: Vec<RootConfig>,
    #[serde(default)]
    pub watch: WatchSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RootConfig {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WatchSettings {
    pub extensions: Vec<String>,
    pub debounce_ms: u64,
}

impl Default for WatchSettings {
    fn default() -> Self {
        WatchSettings {
            extensions: vec![
                "jpg".into(),
                "jpeg".into(),
                "png".into(),
                "tiff".into(),
                "heic".into(),
            ],
            debounce_ms: 2000,
        }
    }
}

impl WatchSettings {
    /// True if `path`'s extension (case-insensitive) is in the allow-list.
    pub fn matches_extension(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                let e = e.to_lowercase();
                self.extensions.iter().any(|allowed| allowed.to_lowercase() == e)
            })
            .unwrap_or(false)
    }
}

pub fn load_config(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading config file {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing config file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_TOML: &str = r#"
server_url = "http://phototag-server:8080"

[[roots]]
name = "pictures"
path = "/path/to/pictures"

[[roots]]
name = "second-library"
path = "/path/to/other/photos"

[watch]
extensions = ["jpg", "jpeg", "png", "tiff", "heic"]
debounce_ms = 2000
"#;

    #[test]
    fn parses_multiple_roots() {
        let config: Config = toml::from_str(EXAMPLE_TOML).unwrap();

        assert_eq!(config.server_url, "http://phototag-server:8080");
        assert_eq!(config.roots.len(), 2);
        assert_eq!(config.roots[0].name, "pictures");
        assert_eq!(config.roots[0].path, PathBuf::from("/path/to/pictures"));
        assert_eq!(config.roots[1].name, "second-library");
        assert_eq!(config.watch.debounce_ms, 2000);
    }

    #[test]
    fn watch_settings_default_when_omitted() {
        let toml = r#"
server_url = "http://phototag-server:8080"

[[roots]]
name = "pictures"
path = "/path/to/pictures"
"#;
        let config: Config = toml::from_str(toml).unwrap();

        assert_eq!(config.watch.debounce_ms, 2000);
        assert!(config.watch.extensions.contains(&"jpg".to_string()));
    }

    #[test]
    fn matches_extension_is_case_insensitive() {
        let settings = WatchSettings::default();

        assert!(settings.matches_extension(Path::new("photo.JPG")));
        assert!(settings.matches_extension(Path::new("photo.heic")));
        assert!(!settings.matches_extension(Path::new("document.pdf")));
    }
}
