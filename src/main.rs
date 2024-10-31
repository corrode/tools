use anyhow::Result;

mod crawl;
mod routes;

use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    pretty_env_logger::init();
    dotenvy::dotenv()?;

    if let Some(cmd) = std::env::args().nth(1) {
        match cmd.as_str() {
            "index" => {
                crawl::index_all().await?;
            }
            "serve" => {
                let repo = Arc::new(crawl::Repository::new("twir.db").await?);

                let app = routes::routes(repo);

                let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
                println!("Listening on http://localhost:3000");
                axum::serve(listener, app).await?;
            }
            _ => println!("Unknown command: {}", cmd),
        }
    } else {
        println!("Usage: twir [index|serve]");
    }

    Ok(())
}
