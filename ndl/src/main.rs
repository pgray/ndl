mod api;
mod config;
mod oauth;
mod tui;

use api::ThreadsClient;
use config::Config;
use std::env;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tui::App;

fn init_logging() {
    let log_dir = Config::dir().expect("Failed to get config directory");
    std::fs::create_dir_all(&log_dir).expect("Failed to create config directory");

    let file_appender = tracing_appender::rolling::never(&log_dir, "ndl.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ndl=info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(non_blocking))
        .init();

    // Keep guard alive for duration of program
    std::mem::forget(_guard);
}

#[tokio::main]
async fn main() {
    // Initialize logging to ~/.config/ndl/ndl.log
    init_logging();
    tracing::info!("ndl starting");

    // Install rustls crypto provider
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("--version") | Some("-V") => {
            print_version();
        }
        Some("login") => {
            tracing::info!("login command");
            if let Err(e) = run_login().await {
                tracing::error!("Login failed: {}", e);
                eprintln!("Login failed: {}", e);
                std::process::exit(1);
            }
        }
        Some("logout") => {
            tracing::info!("logout command");
            if let Err(e) = run_logout() {
                tracing::error!("Logout failed: {}", e);
                eprintln!("Logout failed: {}", e);
                std::process::exit(1);
            }
        }
        Some(cmd) => {
            eprintln!("Unknown command: {}", cmd);
            print_usage();
            std::process::exit(1);
        }
        None => {
            // Check auth status, auto-login if needed
            let config = match Config::load() {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to load config: {}", e);
                    eprintln!("Failed to load config: {}", e);
                    std::process::exit(1);
                }
            };

            if !config.is_authenticated() {
                // Auto-login on first run
                tracing::info!("No token found, starting auto-login");
                println!("No access token found. Starting login...");
                if let Err(e) = run_login().await {
                    tracing::error!("Login failed: {}", e);
                    eprintln!("Login failed: {}", e);
                    std::process::exit(1);
                }
            }

            // Reload config after potential login
            let config = Config::load().expect("Failed to reload config");
            let token = config.access_token.expect("No token after login");
            let client = ThreadsClient::new(token);

            // Fetch initial data
            tracing::debug!("Fetching threads");
            let (client, threads) = match client.get_threads(Some(25)).await {
                Ok(resp) => {
                    tracing::debug!("Fetched {} threads", resp.data.len());
                    (client, resp.data)
                }
                Err(e) if is_auth_error(&e.to_string()) => {
                    tracing::warn!("Token expired or invalid, re-authenticating...");
                    eprintln!("Token expired. Re-authenticating...");
                    if let Err(e) = run_login().await {
                        tracing::error!("Login failed: {}", e);
                        eprintln!("Login failed: {}", e);
                        std::process::exit(1);
                    }
                    // Reload with new token
                    let config = Config::load().expect("Failed to reload config");
                    let token = config.access_token.expect("No token after login");
                    let client = ThreadsClient::new(token);
                    match client.get_threads(Some(25)).await {
                        Ok(resp) => (client, resp.data),
                        Err(e) => {
                            tracing::error!("Failed to fetch threads after re-auth: {}", e);
                            eprintln!("Failed to fetch threads: {}", e);
                            (client, Vec::new())
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to fetch threads: {}", e);
                    eprintln!("Failed to fetch threads: {}", e);
                    (client, Vec::new())
                }
            };

            // Run TUI
            tracing::info!("Starting TUI");
            let mut app = App::new(client, threads);
            if let Err(e) = app.run().await {
                tracing::error!("TUI error: {}", e);
                eprintln!("TUI error: {}", e);
                std::process::exit(1);
            }
            tracing::info!("TUI exited");
        }
    }
}

const DEFAULT_OAUTH_ENDPOINT: &str = "https://ndl.pgray.dev";

async fn run_login() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::load()?;

    // Determine auth server: env var > config > default
    // Empty string means "use local OAuth"
    let auth_server = env::var("NDL_OAUTH_ENDPOINT")
        .ok()
        .or_else(|| config.auth_server.clone())
        .unwrap_or_else(|| DEFAULT_OAUTH_ENDPOINT.to_string());

    let token = if !auth_server.is_empty() {
        // Use hosted auth server
        tracing::info!("Using hosted auth server: {}", auth_server);
        oauth::hosted_login(&auth_server).await?
    } else {
        // Fall back to local OAuth flow
        tracing::info!("Using local OAuth flow");
        let client_id = config
            .client_id
            .clone()
            .or_else(|| env::var("NDL_CLIENT_ID").ok())
            .ok_or("Missing client_id. Set NDL_CLIENT_ID or add to config.")?;

        let client_secret = config
            .client_secret
            .clone()
            .or_else(|| env::var("NDL_CLIENT_SECRET").ok())
            .ok_or("Missing client_secret. Set NDL_CLIENT_SECRET or add to config.")?;

        // Save credentials to config for future use
        config.client_id = Some(client_id.clone());
        config.client_secret = Some(client_secret.clone());

        oauth::login(&client_id, &client_secret).await?
    };

    // Save token to config
    tracing::info!("Login successful, saving token");
    config.access_token = Some(token.access_token);
    config.save()?;

    println!("Token saved to {:?}", Config::path()?);
    Ok(())
}

fn run_logout() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::load()?;
    config.access_token = None;
    config.save()?;
    println!("Logged out. Token removed.");
    Ok(())
}

fn print_version() {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const GIT_VERSION: &str = env!("NDL_GIT_VERSION");
    println!("ndl {} ({})", VERSION, GIT_VERSION);
}

fn print_usage() {
    println!("Usage: ndl [command]");
    println!();
    println!("Commands:");
    println!("  login     Authenticate with Threads");
    println!("  logout    Remove saved access token");
    println!("  --version Show version information");
    println!();
    println!("Run without arguments to start the TUI.");
}

/// Check if an API error indicates an authentication problem
fn is_auth_error(error: &str) -> bool {
    let error_lower = error.to_lowercase();
    error_lower.contains("oauthexception")
        || error_lower.contains("invalid access token")
        || error_lower.contains("session has expired")
        || error_lower.contains("token has expired")
        || error_lower.contains("requires the threads_")  // permission errors
        || error.contains("\"code\":190")  // Facebook/Meta invalid token code
        || error.contains("\"code\":102") // Session expired
}
