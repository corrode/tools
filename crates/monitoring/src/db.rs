use crate::models::{DayBucket, HourBucket, QueryStats, SearchQueryRow, TopQuery};
use anyhow::Result;
use chrono::NaiveDateTime;
use sqlx::Row;

// Monitoring query methods
// -----------------------------------------------------------------------

/// Paginated list of recent search queries, optionally filtered by FTS,
/// source (`"ui"` / `"api"`), and/or content type.
///
/// Returns `(rows, total_count)` so the caller can render pagination.
/// When `search` is `Some`, the query is matched against `events_fts`.
/// When `source` or `content_type` are `Some`, an exact-match WHERE clause
/// is appended.
pub async fn get_query_log(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    search: Option<&str>,
    source: Option<&str>,
    content_type: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<SearchQueryRow>, i64)> {
    // Build the shared WHERE / JOIN fragments and bind values dynamically so
    // every combination of (search, source, content_type) reuses one code
    // path. `QueryBuilder` would also work but the conditions are simple
    // enough that hand-rolling keeps the SQL inspectable.
    let fts_pattern = search
        .filter(|s| !s.is_empty())
        .map(|term| format!("{term}*"));

    let join_fts = if fts_pattern.is_some() {
        "JOIN events_fts ON events_fts.rowid = sq.id"
    } else {
        ""
    };

    let mut conditions: Vec<&str> = Vec::new();
    if fts_pattern.is_some() {
        conditions.push("events_fts MATCH ?");
    }
    if source.is_some() {
        conditions.push("sq.source = ?");
    }
    if content_type.is_some() {
        conditions.push("sq.content_type = ?");
    }
    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM search_queries sq {join_fts} {where_clause}");
    let rows_sql = format!(
        "SELECT sq.id, sq.timestamp, sq.query, sq.result_count, \
                sq.latency_ms, sq.page, sq.referer, \
                sq.content_type, sq.sort_by, sq.start_year, sq.end_year, sq.source \
         FROM search_queries sq {join_fts} {where_clause} \
         ORDER BY sq.timestamp DESC \
         LIMIT ? OFFSET ?"
    );

    let mut count_q = sqlx::query_scalar::<_, i64>(&count_sql);
    if let Some(p) = fts_pattern.as_deref() {
        count_q = count_q.bind(p);
    }
    if let Some(s) = source {
        count_q = count_q.bind(s);
    }
    if let Some(c) = content_type {
        count_q = count_q.bind(c);
    }
    let total: i64 = count_q.fetch_one(pool).await?;

    let mut rows_q = sqlx::query(&rows_sql);
    if let Some(p) = fts_pattern.as_deref() {
        rows_q = rows_q.bind(p);
    }
    if let Some(s) = source {
        rows_q = rows_q.bind(s);
    }
    if let Some(c) = content_type {
        rows_q = rows_q.bind(c);
    }
    let rows = rows_q.bind(limit).bind(offset).fetch_all(pool).await?;

    let parsed = rows
        .iter()
        .map(|row| {
            let ts_str: String = row.get("timestamp");
            let timestamp = parse_monitoring_timestamp(&ts_str);
            SearchQueryRow::new(
                row.get("id"),
                timestamp,
                row.get("query"),
                row.get("result_count"),
                row.get("latency_ms"),
                row.get("page"),
                row.get("referer"),
                row.get("content_type"),
                row.get("sort_by"),
                row.get("start_year"),
                row.get("end_year"),
                row.get("source"),
            )
        })
        .collect();

    Ok((parsed, total))
}

/// Aggregate statistics for the monitoring dashboard gauges.
pub async fn get_query_stats(pool: &sqlx::Pool<sqlx::Sqlite>) -> Result<QueryStats> {
    let gauges = sqlx::query(
            "SELECT \
                COUNT(*) AS total_queries, \
                COUNT(*) FILTER (WHERE timestamp > datetime('now', '-1 hour')) AS queries_last_hour, \
                COUNT(*) FILTER (WHERE timestamp > datetime('now', '-24 hours')) AS queries_last_24h, \
                COALESCE(AVG(latency_ms), 0) AS avg_latency_ms, \
                COALESCE(AVG(result_count), 0) AS avg_result_count, \
                COALESCE( \
                    CAST(SUM(CASE WHEN result_count = 0 THEN 1 ELSE 0 END) AS REAL) \
                    / NULLIF(COUNT(*), 0), 0 \
                ) AS zero_result_rate \
             FROM search_queries",
        )
        .fetch_one(pool)
        .await?;

    let total_queries: i64 = gauges.get("total_queries");

    // p50 — median latency
    let p50_latency_ms: i64 = sqlx::query_scalar(
        "SELECT COALESCE(latency_ms, 0) FROM search_queries \
             WHERE latency_ms IS NOT NULL \
             ORDER BY latency_ms \
             LIMIT 1 OFFSET (SELECT COUNT(*) / 2 FROM search_queries WHERE latency_ms IS NOT NULL)",
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or(0);

    // p99 — 99th-percentile latency
    let p99_latency_ms: i64 = sqlx::query_scalar(
            "SELECT COALESCE(latency_ms, 0) FROM search_queries \
             WHERE latency_ms IS NOT NULL \
             ORDER BY latency_ms \
             LIMIT 1 OFFSET (SELECT COUNT(*) * 99 / 100 FROM search_queries WHERE latency_ms IS NOT NULL)",
        )
        .fetch_optional(pool)
        .await?
        .unwrap_or(0);

    let error_count_last_hour = get_error_count(pool, 1).await?;

    Ok(QueryStats::new(
        total_queries,
        gauges.get("queries_last_hour"),
        gauges.get("queries_last_24h"),
        gauges.get("avg_latency_ms"),
        p50_latency_ms,
        p99_latency_ms,
        gauges.get("avg_result_count"),
        gauges.get("zero_result_rate"),
        error_count_last_hour,
    ))
}

/// Top N queries by frequency, optionally within a time window.
pub async fn get_top_queries(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    since: Option<NaiveDateTime>,
    limit: i64,
) -> Result<Vec<TopQuery>> {
    let rows = if let Some(since) = since {
        let since_str = since.format("%Y-%m-%d %H:%M:%S%.6f").to_string();
        sqlx::query(
            "SELECT query, COUNT(*) AS count, \
                        COALESCE(AVG(result_count), 0) AS avg_result_count \
                 FROM search_queries \
                 WHERE timestamp > ? \
                 GROUP BY query \
                 ORDER BY count DESC \
                 LIMIT ?",
        )
        .bind(&since_str)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT query, COUNT(*) AS count, \
                        COALESCE(AVG(result_count), 0) AS avg_result_count \
                 FROM search_queries \
                 GROUP BY query \
                 ORDER BY count DESC \
                 LIMIT ?",
        )
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    let results = rows
        .iter()
        .map(|row| {
            TopQuery::new(
                row.get("query"),
                row.get("count"),
                row.get("avg_result_count"),
            )
        })
        .collect();

    Ok(results)
}

/// Hourly event counts for the last `hours` hours.
pub async fn get_hourly_histogram(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    hours: i64,
) -> Result<Vec<HourBucket>> {
    let rows = sqlx::query(
        "SELECT strftime('%Y-%m-%d %H:00:00', timestamp) AS hour, \
                    COUNT(*) AS count \
             FROM search_queries \
             WHERE timestamp > datetime('now', '-' || ? || ' hours') \
             GROUP BY hour \
             ORDER BY hour",
    )
    .bind(hours)
    .fetch_all(pool)
    .await?;

    let buckets = rows
        .iter()
        .map(|row| HourBucket::new(row.get("hour"), row.get("count")))
        .collect();

    Ok(buckets)
}

/// Daily event counts for the last `days` days.
pub async fn get_daily_histogram(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    days: i64,
) -> Result<Vec<DayBucket>> {
    let rows = sqlx::query(
        "SELECT strftime('%Y-%m-%d', timestamp) AS day, \
                    COUNT(*) AS count \
             FROM search_queries \
             WHERE timestamp > datetime('now', '-' || ? || ' days') \
             GROUP BY day \
             ORDER BY day",
    )
    .bind(days)
    .fetch_all(pool)
    .await?;

    let buckets = rows
        .iter()
        .map(|row| DayBucket::new(row.get("day"), row.get("count")))
        .collect();

    Ok(buckets)
}

/// Top queries that returned zero results, ranked by frequency.
pub async fn get_zero_result_queries(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    limit: i64,
) -> Result<Vec<TopQuery>> {
    let rows = sqlx::query(
        "SELECT query, COUNT(*) AS count, 0.0 AS avg_result_count \
             FROM search_queries \
             WHERE result_count = 0 \
             GROUP BY query \
             ORDER BY count DESC \
             LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let results = rows
        .iter()
        .map(|row| {
            TopQuery::new(
                row.get("query"),
                row.get("count"),
                row.get("avg_result_count"),
            )
        })
        .collect();

    Ok(results)
}

/// Count of `WARN` + `ERROR` events in the last `since_hours` hours.
///
/// Queries the raw `events` table (not the `search_queries` view) so it
/// captures errors from all event types, not just search requests.
pub async fn get_error_count(pool: &sqlx::Pool<sqlx::Sqlite>, since_hours: i64) -> Result<i64> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM events \
             WHERE level IN ('WARN', 'ERROR') \
               AND timestamp > datetime('now', '-' || ? || ' hours')",
    )
    .bind(since_hours)
    .fetch_one(pool)
    .await?;

    Ok(count)
}

/// Parse a monitoring timestamp string into [`NaiveDateTime`].
///
/// Handles the `YYYY-MM-DD HH:MM:SS.ffffff` format written by the
/// `SqliteLayer`, falling back to a zero epoch on parse failure.
fn parse_monitoring_timestamp(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
        .unwrap_or_else(|_| NaiveDateTime::default())
}
