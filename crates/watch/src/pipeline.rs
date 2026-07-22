use std::path::Path;

use anyhow::Result;

use crate::client::TaggerClient;
use crate::exif;

#[derive(Debug)]
pub enum TagOutcome {
    Tagged(Vec<String>),
    AlreadyTagged,
}

/// Tags a single file: skips it if it already has keywords, otherwise
/// calls `phototag-server` and writes the result into the file's metadata.
pub async fn tag_one_file(path: &Path, client: &TaggerClient) -> Result<TagOutcome> {
    if exif::has_keywords(path).await? {
        return Ok(TagOutcome::AlreadyTagged);
    }
    let keywords = client.tag_image(path).await?;
    exif::write_keywords(path, &keywords).await?;
    Ok(TagOutcome::Tagged(keywords))
}
