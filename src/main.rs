mod app;
mod github_api;
mod parser;

fn main() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .without_time()
        .with_target(false)
        .try_init()
        .ok();
    if let Err(err) = app::run() {
        tracing::error!(?err);
        std::process::exit(1);
    }
}
