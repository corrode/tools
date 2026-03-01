//! Dashboard handler for the monitoring backend.
//!
//! Serves the main dashboard page with gauge cards, bar charts, top queries,
//! and zero-result queries.

use askama::Template;
use axum::{extract::State, response::Html};
use charming::{
    Chart,
    component::{Axis, Grid, Title},
    element::{AxisLabel, AxisType, ItemStyle, Label, LabelPosition, TextStyle},
    series::Bar,
};

use crate::error::AppError;
use crate::models::{DayBucket, HourBucket, QueryStats, TopQuery};

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

#[derive(Template)]
#[template(path = "dashboard.html")]
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
pub async fn dashboard(
    State(pool): State<sqlx::Pool<sqlx::Sqlite>>,
) -> Result<Html<String>, AppError> {
    let stats = crate::db::get_query_stats(&pool).await?;
    let hourly = crate::db::get_hourly_histogram(&pool, 48).await?;
    let daily = crate::db::get_daily_histogram(&pool, 30).await?;
    let top_queries = crate::db::get_top_queries(&pool, None, 20).await?;
    let zero_result_queries = crate::db::get_zero_result_queries(&pool, 20).await?;

    let hourly_chart_json = generate_hourly_chart(&hourly);
    let daily_chart_json = generate_daily_chart(&daily);

    let template = DashboardTemplate {
        stats,
        hourly_chart_json,
        daily_chart_json,
        top_queries,
        zero_result_queries,
    };

    Ok(template.render().map(Html)?)
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
