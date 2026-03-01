//! Dashboard handler for the monitoring backend.
//!
//! Serves the main dashboard page with gauge cards, bar charts, top queries,
//! and zero-result queries.

use askama::Template;
use axum::{extract::State, http::StatusCode, response::Html};
use charming::{
    Chart,
    component::{Axis, Grid, Title},
    element::{AxisLabel, AxisType, ItemStyle, Label, LabelPosition, TextStyle},
    series::Bar,
};
use std::sync::Arc;

use crate::error::AppErrorExt;
use storage::Repository;
use types::monitoring::{DayBucket, HourBucket, QueryStats, TopQuery};

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "monitoring/dashboard.html")]
struct DashboardTemplate {
    stats: QueryStats,
    hourly_chart_json: String,
    daily_chart_json: String,
    top_queries: Vec<TopQuery>,
    zero_result_queries: Vec<TopQuery>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Renders the full monitoring dashboard page.
pub(crate) async fn dashboard(
    State(repo): State<Arc<Repository>>,
) -> Result<Html<String>, StatusCode> {
    let stats = repo.get_query_stats().await.into_internal_server_error()?;
    let hourly = repo
        .get_hourly_histogram(48)
        .await
        .into_internal_server_error()?;
    let daily = repo
        .get_daily_histogram(30)
        .await
        .into_internal_server_error()?;
    let top_queries = repo
        .get_top_queries(None, 20)
        .await
        .into_internal_server_error()?;
    let zero_result_queries = repo
        .get_zero_result_queries(20)
        .await
        .into_internal_server_error()?;

    let hourly_chart_json = generate_hourly_chart(&hourly);
    let daily_chart_json = generate_daily_chart(&daily);

    let template = DashboardTemplate {
        stats,
        hourly_chart_json,
        daily_chart_json,
        top_queries,
        zero_result_queries,
    };

    template.render().map(Html).into_internal_server_error()
}

// ---------------------------------------------------------------------------
// Chart generation (charming / ECharts)
// ---------------------------------------------------------------------------

/// Generate an ECharts bar chart JSON string for hourly query counts.
fn generate_hourly_chart(buckets: &[HourBucket]) -> String {
    let x_data: Vec<String> = buckets
        .iter()
        .map(|b| {
            // Show only the hour portion for readability: "14:00"
            if b.hour.len() >= 16 {
                b.hour[11..16].to_string()
            } else {
                b.hour.clone()
            }
        })
        .collect();

    let y_data: Vec<i64> = buckets.iter().map(|b| b.count).collect();

    let chart = Chart::new()
        .title(
            Title::new()
                .text("Queries Per Hour (last 48h)")
                .left("center")
                .text_style(TextStyle::new().color("#e0e0e0").font_size(16)),
        )
        .grid(
            Grid::new()
                .left("5%")
                .right("5%")
                .bottom("15%")
                .top("15%")
                .contain_label(true),
        )
        .x_axis(
            Axis::new()
                .type_(AxisType::Category)
                .data(x_data)
                .axis_label(AxisLabel::new().rotate(45)),
        )
        .y_axis(Axis::new().type_(AxisType::Value))
        .series(
            Bar::new()
                .data(y_data)
                .item_style(
                    ItemStyle::new()
                        .color("var(--color-primary)")
                        .border_radius(4.0),
                )
                .label(
                    Label::new()
                        .show(false)
                        .position(LabelPosition::Top)
                        .color("#999")
                        .font_size(10),
                ),
        );

    serde_json::to_string(&chart).unwrap_or_else(|_| "{}".to_string())
}

/// Generate an ECharts bar chart JSON string for daily query counts.
fn generate_daily_chart(buckets: &[DayBucket]) -> String {
    let x_data: Vec<String> = buckets
        .iter()
        .map(|b| {
            // Show short date: "02-27"
            if b.day.len() >= 10 {
                b.day[5..10].to_string()
            } else {
                b.day.clone()
            }
        })
        .collect();

    let y_data: Vec<i64> = buckets.iter().map(|b| b.count).collect();

    let chart = Chart::new()
        .title(
            Title::new()
                .text("Queries Per Day (last 30d)")
                .left("center")
                .text_style(TextStyle::new().color("#e0e0e0").font_size(16)),
        )
        .grid(
            Grid::new()
                .left("5%")
                .right("5%")
                .bottom("15%")
                .top("15%")
                .contain_label(true),
        )
        .x_axis(
            Axis::new()
                .type_(AxisType::Category)
                .data(x_data)
                .axis_label(AxisLabel::new().rotate(45)),
        )
        .y_axis(Axis::new().type_(AxisType::Value))
        .series(
            Bar::new()
                .data(y_data)
                .item_style(
                    ItemStyle::new()
                        .color("var(--color-primary)")
                        .border_radius(4.0),
                )
                .label(
                    Label::new()
                        .show(true)
                        .position(LabelPosition::Top)
                        .color("#999")
                        .font_size(10),
                ),
        );

    serde_json::to_string(&chart).unwrap_or_else(|_| "{}".to_string())
}
