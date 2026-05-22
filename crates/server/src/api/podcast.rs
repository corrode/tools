//! `GET /api/v1/podcasts/{id}` — full podcast episode with transcript.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use storage::Repository;

use crate::api::dto::PodcastDetail;
use crate::api::error::ApiError;

/// Fetch a single podcast episode by its numeric identifier.
///
/// The response includes episode metadata plus the full raw transcript text.
/// Transcripts use WebVTT-style `<v Speaker>` cues when speaker labels are
/// available; clients can parse them or render the text as-is.
///
/// Returns `404` if the episode does not exist.
#[utoipa::path(
    get,
    path = "/podcasts/{id}",
    tag = "podcasts",
    params(
        ("id" = i64, Path, description = "Podcast episode database identifier", example = 42),
    ),
    responses(
        (status = 200, description = "Podcast episode", body = PodcastDetail),
        (status = 404, description = "Episode not found", body = ApiError),
        (status = 500, description = "Internal server error", body = ApiError),
    ),
)]
pub(crate) async fn get_podcast(
    Path(id): Path<i64>,
    State(repo): State<Arc<Repository>>,
) -> Result<Json<PodcastDetail>, ApiError> {
    let episode = repo
        .get_podcast_episode_by_id(id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("Podcast episode {id} not found")))?;

    let guests = repo
        .get_podcast_episode_guests(id)
        .await
        .unwrap_or_default();

    Ok(Json(PodcastDetail::from_episode(episode, guests)))
}
