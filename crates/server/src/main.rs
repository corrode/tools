use anyhow::Result;

mod routes;

use storage::Repository;
pub use types::Entry;
use types::SQLITE_DB_PATH;

use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    dotenvy::dotenv()?;

    let repo = Arc::new(Repository::new(SQLITE_DB_PATH).await?);

    let app = routes::routes(repo);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Listening on http://localhost:3000");
    Ok(axum::serve(listener, app).await?)
}
