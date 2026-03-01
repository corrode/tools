use askama::Template;
use axum::{extract::State, http::HeaderMap, response::Html};
use charming::{
    Chart,
    component::{Axis, Grid, Title},
    element::{AxisLabel, AxisType, ItemStyle, Label, LabelPosition, TextStyle},
    series::Bar,
};
use std::sync::Arc;

use crate::error::AppErrorExt;
use storage::Repository;
use types::Stats;

#[derive(Template)]
#[template(path = "stats.html")]
struct StatsTemplate {
    stats: Stats,
    chart_json: String,
}

/// Handler for the stats page
pub(crate) async fn stats(
    headers: HeaderMap,
    State(repo): State<Arc<Repository>>,
) -> Result<Html<String>, axum::http::StatusCode> {
    let referer = headers
        .get(axum::http::header::REFERER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");

    tracing::info!(is_monitoring = true, referer, "Stats page viewed");

    let stats = repo.get_stats().await.into_internal_server_error()?;

    // Generate chart configuration
    let chart_json = generate_chart(&stats);

    let template = StatsTemplate { stats, chart_json };
    template.render().map(Html).into_internal_server_error()
}

fn generate_chart(stats: &Stats) -> String {
    let x_data: Vec<String> = stats
        .articles
        .per_month
        .iter()
        .map(|m| m.year_month.clone())
        .collect();

    let y_data: Vec<i64> = stats.articles.per_month.iter().map(|m| m.count).collect();

    let chart = Chart::new()
        .title(
            Title::new()
                .text("Articles Per Month")
                .left("center")
                .text_style(TextStyle::new().color("#e0e0e0").font_size(20)),
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
                .item_style(ItemStyle::new().color("#ff6b35").border_radius(4.0))
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
