use axum::{
    body::Bytes,
    extract::State,
    http::{header::CONTENT_TYPE, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use phototag_common::TagResponse;
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

pub async fn tag_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if body.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "empty request body".into(),
            }),
        )
            .into_response();
    }

    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    match state.gateway.extract_keywords(&body, &content_type).await {
        Ok(keywords) => (StatusCode::OK, Json(TagResponse { keywords })).into_response(),
        Err(e) => {
            tracing::warn!("tag request failed: {e:#}");
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorBody {
                    error: e.to_string(),
                }),
            )
                .into_response()
        }
    }
}
