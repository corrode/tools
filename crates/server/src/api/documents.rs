//! Public JSON API endpoints for direct document access:
//!
//! - `GET  /api/v1/documents/{doc_id}` — full content for one document.
//! - `POST /api/v1/documents:batch`    — full content for up to 25 documents.
//! - `GET  /api/v1/documents/{doc_id}/search` — ranked passages inside one document.
//!
//! These endpoints are designed for LLM "deep research" clients that need to
//! ground their reasoning in the actual document body, not just a snippet,
//! without re-fetching the source page from the open web.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use storage::Repository;
use types::{DocumentKind, DocumentRef};
use utoipa::IntoParams;

use crate::api::dto::{
    BatchDocumentsRequest, BatchDocumentsResponse, DocumentDetail, DocumentSearchResponse,
    PassageHit,
};
use crate::api::error::ApiError;
use crate::api::passages;

/// Maximum number of passages a single `/documents/{doc_id}/search` request
/// may return. Higher caps just waste bandwidth — LLM clients rarely need
/// more than a handful of excerpts per document.
const MAX_PASSAGES: usize = 20;
/// Default number of passages when the client does not specify `max`.
const DEFAULT_PASSAGES: usize = 5;
/// Passage width in characters. Picked to fit comfortably in a single LLM
/// "snippet" budget while keeping enough surrounding context to disambiguate
/// matches.
const PASSAGE_WINDOW: usize = 400;

/// Parses a `doc_id` path segment like `article:42` into a [`DocumentRef`],
/// or returns a `400 invalid_params` if the string is malformed.
fn parse_doc_id(raw: &str) -> Result<DocumentRef, ApiError> {
    raw.parse::<DocumentRef>()
        .map_err(|err| ApiError::invalid_params(err.to_string()))
}

/// Internal helper: fetch a document by `doc_id`, dispatching to the right
/// per-kind storage call. Returns `Ok(None)` when the document does not exist.
async fn load_document(
    repo: &Repository,
    doc_id: DocumentRef,
) -> Result<Option<DocumentDetail>, ApiError> {
    match doc_id.kind {
        DocumentKind::Article => Ok(repo
            .get_article_by_id(doc_id.id)
            .await?
            .map(DocumentDetail::from_article)),
        DocumentKind::Video => Ok(repo
            .get_video_by_id(doc_id.id)
            .await?
            .map(DocumentDetail::from_video)),
        DocumentKind::Talk => Ok(repo
            .get_talk_by_id(doc_id.id)
            .await?
            .map(DocumentDetail::from_talk)),
        DocumentKind::Podcast => {
            let Some(episode) = repo.get_podcast_episode_by_id(doc_id.id).await? else {
                return Ok(None);
            };
            let guests = repo
                .get_podcast_episode_guests(doc_id.id)
                .await
                .unwrap_or_default();
            Ok(Some(DocumentDetail::from_podcast(episode, guests)))
        }
        DocumentKind::Research => Ok(repo
            .get_research_paper_by_id(doc_id.id)
            .await?
            .map(DocumentDetail::from_research)),
    }
}

/// Fetches one indexed document by its stable `doc_id` (`{kind}:{id}`).
///
/// The response is a tagged union on `kind` and always includes the full
/// document body in `content.text`, plus a heuristic `token_estimate` and
/// `char_count` for budgeting.
///
/// This is the primary "let an LLM read the source" endpoint: instead of
/// re-fetching the article/transcript from the open web and re-parsing it,
/// callers can pull the canonical text we already have indexed.
///
/// Returns `404` if the document does not exist.
#[utoipa::path(
    get,
    path = "/documents/{doc_id}",
    tag = "documents",
    params(
        ("doc_id" = DocumentRef, Path, description = "Kind-prefixed document id, e.g. `article:42`"),
    ),
    responses(
        (status = 200, description = "Document found", body = DocumentDetail),
        (status = 400, description = "Malformed doc_id", body = ApiError),
        (status = 404, description = "Document not found", body = ApiError),
        (status = 500, description = "Internal server error", body = ApiError),
    ),
)]
pub(crate) async fn get_document(
    Path(raw_id): Path<String>,
    State(repo): State<Arc<Repository>>,
) -> Result<Json<DocumentDetail>, ApiError> {
    let doc_id = parse_doc_id(&raw_id)?;
    let doc = load_document(&repo, doc_id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("document `{doc_id}` not found")))?;
    Ok(Json(doc))
}

/// Fetches several documents in one round trip.
///
/// Deep-research agents almost always fan out from a single search result to
/// 5–20 promising documents. Doing that as N separate HTTP requests adds
/// latency and connection overhead for no benefit, so this endpoint takes a
/// list of identifiers and returns them all in one shot.
///
/// The response splits hits into `documents` (preserving the request order,
/// minus missing entries) and `missing` (identifiers that resolved to no
/// document). This lets clients reconcile their request with the response
/// without having to walk a sparse array.
///
/// A duplicate id in `ids` is fetched once but appears twice in `documents`.
/// The cap is 25 ids per request.
#[utoipa::path(
    post,
    path = "/documents:batch",
    tag = "documents",
    request_body = BatchDocumentsRequest,
    responses(
        (status = 200, description = "Batch results", body = BatchDocumentsResponse),
        (status = 400, description = "Empty or oversized id list", body = ApiError),
        (status = 500, description = "Internal server error", body = ApiError),
    ),
)]
pub(crate) async fn batch_documents(
    State(repo): State<Arc<Repository>>,
    Json(body): Json<BatchDocumentsRequest>,
) -> Result<Json<BatchDocumentsResponse>, ApiError> {
    if body.ids.is_empty() {
        return Err(ApiError::invalid_params(
            "batch request must contain at least one id",
        ));
    }
    if body.ids.len() > Repository::MAX_BATCH_DOCUMENTS {
        return Err(ApiError::invalid_params(format!(
            "batch request has {} ids, maximum is {}",
            body.ids.len(),
            Repository::MAX_BATCH_DOCUMENTS
        )));
    }

    // Deduplicate while preserving the first-seen order, so we don't hit the
    // DB twice for the same `doc_id`. We then re-expand the cached lookups
    // back into the response in the caller's original order.
    let mut unique: Vec<DocumentRef> = Vec::with_capacity(body.ids.len());
    for id in &body.ids {
        if !unique.contains(id) {
            unique.push(*id);
        }
    }

    let mut loaded: Vec<(DocumentRef, Option<DocumentDetail>)> = Vec::with_capacity(unique.len());
    for id in unique {
        let doc = load_document(&repo, id).await?;
        loaded.push((id, doc));
    }

    let mut documents = Vec::with_capacity(body.ids.len());
    let mut missing = Vec::new();
    for id in &body.ids {
        match loaded.iter().find(|(d, _)| d == id) {
            Some((_, Some(doc))) => {
                // Clone because the same `doc_id` may appear twice in the request.
                documents.push(doc.clone());
            }
            _ => missing.push(*id),
        }
    }
    Ok(Json(BatchDocumentsResponse { documents, missing }))
}

/// Query string for `GET /documents/{doc_id}/search`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct DocumentSearchParams {
    /// Phrase or words to search for. Required.
    #[param(example = "lifetime elision")]
    pub q: String,
    /// Maximum number of passages to return. Defaults to 5, capped at 20.
    #[param(example = 5, minimum = 1, maximum = 20)]
    pub max: Option<usize>,
}

/// Searches inside a single document, returning ranked text passages.
///
/// Use this when an LLM has identified a promising document via `/search`
/// or `/documents/{doc_id}` and now wants to pull just the parts relevant
/// to a follow-up question, without paying the token cost of the whole body.
///
/// The query is treated as a bag of whitespace-separated terms; quotes have
/// no special meaning here (unlike `/search`). Matching is case-insensitive
/// substring matching, so `async` matches `asynchronous`. Returned passages
/// are non-overlapping, ranked by match count, and each carries character
/// offsets that index into `content.text` from `GET /documents/{doc_id}`.
///
/// Returns `404` if the document does not exist, `400` if `q` is empty.
#[utoipa::path(
    get,
    path = "/documents/{doc_id}/search",
    tag = "documents",
    params(
        ("doc_id" = DocumentRef, Path, description = "Kind-prefixed document id"),
        DocumentSearchParams,
    ),
    responses(
        (status = 200, description = "Passages from the document", body = DocumentSearchResponse),
        (status = 400, description = "Empty query or malformed id", body = ApiError),
        (status = 404, description = "Document not found", body = ApiError),
        (status = 500, description = "Internal server error", body = ApiError),
    ),
)]
pub(crate) async fn search_in_document(
    Path(raw_id): Path<String>,
    Query(params): Query<DocumentSearchParams>,
    State(repo): State<Arc<Repository>>,
) -> Result<Json<DocumentSearchResponse>, ApiError> {
    let doc_id = parse_doc_id(&raw_id)?;
    let query = params.q.trim();
    if query.is_empty() {
        return Err(ApiError::invalid_params("query `q` must be non-empty"));
    }
    let max = params
        .max
        .unwrap_or(DEFAULT_PASSAGES)
        .clamp(1, MAX_PASSAGES);

    let doc = load_document(&repo, doc_id)
        .await?
        .ok_or_else(|| ApiError::not_found(format!("document `{doc_id}` not found")))?;

    let terms: Vec<&str> = query.split_whitespace().collect();
    let passages: Vec<PassageHit> = passages::extract(doc.body(), &terms, max, PASSAGE_WINDOW)
        .into_iter()
        .map(PassageHit::from)
        .collect();

    let body = doc.body();
    let char_count = u32::try_from(body.chars().count()).unwrap_or(u32::MAX);
    let token_estimate = passages::estimate_tokens(body);

    let returned = passages.len();
    Ok(Json(DocumentSearchResponse {
        doc_id: doc.doc_id(),
        query: query.to_string(),
        char_count,
        token_estimate,
        returned,
        passages,
    }))
}
