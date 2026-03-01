//! Tracing layer that persists monitoring events to SQLite.
//!
//! Only events with `target: "monitoring"` are captured. All other events pass
//! through untouched (they still reach the `fmt` layer for stdout logging).
//!
//! The layer sends [`EventRecord`]s through a bounded `mpsc` channel to a
//! background [`drain_loop`] task that batch-INSERTs them into the `events`
//! table. This design keeps the hot path (the `on_event` callback) allocation-
//! light and non-blocking.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;

use sqlx::{Pool, QueryBuilder, Sqlite};
use tokio::sync::mpsc;
use tracing::Subscriber;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

/// Channel capacity — large enough to absorb short bursts without dropping
/// events, small enough to bound memory usage.
const CHANNEL_CAPACITY: usize = 1024;

/// Maximum number of events accumulated before flushing a batch INSERT.
const BATCH_SIZE: usize = 64;

/// Number of inserts between retention-cleanup passes.
const CLEANUP_INTERVAL: u64 = 5_000;

/// Maximum number of rows kept in the `events` table.
const MAX_ROWS: i64 = 100_000;

// ---------------------------------------------------------------------------
// EventRecord
// ---------------------------------------------------------------------------

/// A self-contained snapshot of a tracing event, ready to be inserted into the
/// `events` table. Produced on the hot path and consumed by [`drain_loop`].
#[derive(Debug)]
pub struct EventRecord {
    /// UTC timestamp formatted as `YYYY-MM-DD HH:MM:SS.ffffff` (SQLite-native).
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Log level as an uppercase string (`INFO`, `WARN`, `ERROR`).
    pub level: String,
    /// The human-readable message (the `message` field of the tracing event).
    pub message: String,
    /// Remaining structured fields serialised as a JSON object.
    pub fields: String,
}

// ---------------------------------------------------------------------------
// FieldVisitor
// ---------------------------------------------------------------------------

/// Visits the fields of a tracing event, collecting them into a map of JSON
/// values. The `message` field is extracted separately by the caller.
struct FieldVisitor {
    message: String,
    fields: HashMap<String, serde_json::Value>,
}

impl FieldVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
            fields: HashMap::new(),
        }
    }
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.fields.insert(
                field.name().to_string(),
                serde_json::Value::String(format!("{value:?}")),
            );
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }
}


// ---------------------------------------------------------------------------
// SqliteLayer
// ---------------------------------------------------------------------------

/// A [`tracing_subscriber::Layer`] that captures events with
/// `target: "monitoring"` and forwards them to a background writer task via a
/// bounded channel.
///
/// Construct with [`SqliteLayer::new`], which returns the layer **and** a
/// future that the caller must spawn (the drain loop).
#[derive(Debug)]
pub struct SqliteLayer {
    tx: mpsc::Sender<EventRecord>,
}

impl SqliteLayer {
    /// Create the layer and the background drain task.
    ///
    /// The caller **must** `tokio::spawn` the returned future; it runs the
    /// batch-INSERT loop and will exit when the channel is closed (i.e. when
    /// the layer is dropped).
    pub fn new(pool: Pool<Sqlite>) -> (Self, impl Future<Output = ()>) {
        let (tx, rx) = mpsc::channel::<EventRecord>(CHANNEL_CAPACITY);
        let task = drain_loop(rx, pool);
        (Self { tx }, task)
    }
}

impl<S> Layer<S> for SqliteLayer
where
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();

        // Extract fields via visitor
        let mut visitor = FieldVisitor::new();
        event.record(&mut visitor);


        let fields_json =
            serde_json::to_string(&visitor.fields).unwrap_or_else(|_| "{}".to_string());

        let record = EventRecord {
            timestamp: chrono::Utc::now(),
            level: meta.level().to_string(),
            message: visitor.message,
            fields: fields_json,
        };

        // Bounded channel — silently drop on backpressure (never block the
        // caller). This is intentional: losing a monitoring event is
        // preferable to slowing down request handling.
        let _ = self.tx.try_send(record);
    }
}

// ---------------------------------------------------------------------------
// Drain loop + helpers
// ---------------------------------------------------------------------------

/// Background task that drains the channel and batch-INSERTs into SQLite.
///
/// Exits when the sending half of the channel is dropped (i.e. the layer is
/// gone, typically at shutdown).
async fn drain_loop(mut rx: mpsc::Receiver<EventRecord>, pool: Pool<Sqlite>) {
    let mut buf: Vec<EventRecord> = Vec::with_capacity(BATCH_SIZE);
    let mut inserts_since_cleanup: u64 = 0;

    loop {
        let count = rx.recv_many(&mut buf, BATCH_SIZE).await;
        if count == 0 {
            break; // Channel closed
        }

        if let Err(e) = batch_insert(&pool, &buf).await {
            eprintln!("monitoring: batch insert failed: {e}");
        }

        inserts_since_cleanup += count as u64;
        buf.clear();

        // Periodic retention cleanup to cap table growth.
        if inserts_since_cleanup >= CLEANUP_INTERVAL {
            inserts_since_cleanup = 0;
            if let Err(e) = enforce_retention(&pool, MAX_ROWS).await {
                eprintln!("monitoring: retention cleanup failed: {e}");
            }
        }
    }

    if !buf.is_empty()
        && let Err(e) = batch_insert(&pool, &buf).await {
            eprintln!("monitoring: final batch insert failed: {e}");
        }
}

/// Batch-INSERT a slice of events using `sqlx::QueryBuilder::push_values`.
async fn batch_insert(pool: &Pool<Sqlite>, events: &[EventRecord]) -> Result<(), sqlx::Error> {
    if events.is_empty() {
        return Ok(());
    }

    let mut qb: QueryBuilder<Sqlite> =
        QueryBuilder::new("INSERT INTO events (timestamp, level, message, fields) ");

    qb.push_values(events, |mut b, e| {
        let ts = e.timestamp.format("%Y-%m-%d %H:%M:%S%.6f").to_string();
        b.push_bind(ts)
            .push_bind(&e.level)
            .push_bind(&e.message)
            .push_bind(&e.fields);
    });

    qb.build().execute(pool).await?;
    Ok(())
}

/// Delete old rows so the table never exceeds `max_rows`.
async fn enforce_retention(pool: &Pool<Sqlite>, max_rows: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM events WHERE id NOT IN \
         (SELECT id FROM events ORDER BY id DESC LIMIT ?)",
    )
    .bind(max_rows)
    .execute(pool)
    .await?;
    Ok(())
}

