use serde::{Deserialize, Serialize};

/// Response body for `phototag-server`'s `POST /tag`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagResponse {
    pub keywords: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_response_round_trips_through_json() {
        let original = TagResponse {
            keywords: vec!["dog".to_string(), "beach".to_string()],
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: TagResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }
}
