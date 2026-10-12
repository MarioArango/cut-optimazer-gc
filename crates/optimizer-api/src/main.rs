//! Rust-only HTTP entry point; configurable entirely by environment variables.

#[tokio::main]
async fn main() {
    let config = match optimizer_api::ApiConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("optimizer-api configuration error: {error}");
            std::process::exit(2);
        }
    };
    if let Err(error) = optimizer_api::serve(config).await {
        eprintln!("optimizer-api server error: {error}");
        std::process::exit(1);
    }
}
