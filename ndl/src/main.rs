mod api;
mod bluesky;
mod config;
mod oauth;
mod platform;
mod tui;

use api::ThreadsClient;
use bluesky::BlueskyClient;
use config::Config;
use platform::{Platform, SocialClient};
use std::collections::HashMap;
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
            // Check if a platform is specified
            let platform = args.get(2).map(|s| s.as_str());
            match platform {
                Some("bluesky") | Some("bsky") => {
                    tracing::info!("login bluesky command");
                    if let Err(e) = run_bluesky_login().await {
                        tracing::error!("Bluesky login failed: {}", e);
                        eprintln!("Bluesky login failed: {}", e);
                        std::process::exit(1);
                    }
                }
                Some("threads") | None => {
                    tracing::info!("login threads command");
                    if let Err(e) = run_login().await {
                        tracing::error!("Login failed: {}", e);
                        eprintln!("Login failed: {}", e);
                        std::process::exit(1);
                    }
                }
                Some(platform) => {
                    eprintln!("Unknown platform: {}", platform);
                    eprintln!("Supported platforms: threads, bluesky");
                    std::process::exit(1);
                }
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
            if let Err(e) = run_tui().await {
                tracing::error!("TUI error: {}", e);
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

const DEFAULT_OAUTH_ENDPOINT: &str = "https://ndl.pgray.dev";

async fn run_login() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::load()?;

    // Preserve existing Bluesky config
    let existing_bluesky = config.bluesky.clone();
    tracing::debug!(
        "Loaded config - has_bluesky: {}, has_threads: {}",
        config.has_bluesky(),
        config.has_threads()
    );

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

    // Ensure Bluesky config is preserved
    if config.bluesky.is_none() && existing_bluesky.is_some() {
        tracing::warn!("Bluesky config was lost during login, restoring");
        config.bluesky = existing_bluesky;
    }

    tracing::debug!(
        "Saving config - has_bluesky: {}, has_threads: {}",
        config.has_bluesky(),
        config.has_threads()
    );
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

async fn run_bluesky_login() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{self, Write};

    println!("Bluesky Login");
    println!("=============");
    println!();
    println!("You can use your handle (e.g., user.bsky.social) or email as identifier.");
    println!("For enhanced security, consider using an app-specific password:");
    println!("https://bsky.app/settings/app-passwords");
    println!();

    // Prompt for identifier
    print!("Identifier (handle or email): ");
    io::stdout().flush()?;
    let mut identifier = String::new();
    io::stdin().read_line(&mut identifier)?;
    let identifier = identifier.trim().to_string();

    if identifier.is_empty() {
        return Err("Identifier cannot be empty".into());
    }

    // Prompt for password
    print!("Password (or app password): ");
    io::stdout().flush()?;
    let mut password = String::new();
    io::stdin().read_line(&mut password)?;
    let password = password.trim().to_string();

    if password.is_empty() {
        return Err("Password cannot be empty".into());
    }

    // Test login
    println!();
    println!("Authenticating...");
    match BlueskyClient::login(&identifier, &password).await {
        Ok(client) => {
            println!("✓ Authentication successful!");

            // Get and save session data
            let session = client.get_session().await.ok();

            // Save to config (preserving existing Threads config)
            let mut config = Config::load()?;
            let existing_threads = config.access_token.clone();
            tracing::debug!(
                "Loaded config - has_bluesky: {}, has_threads: {}",
                config.has_bluesky(),
                config.has_threads()
            );

            config.bluesky = Some(config::BlueskyConfig {
                identifier,
                password,
                session,
            });

            // Ensure Threads config is preserved
            if config.access_token.is_none() && existing_threads.is_some() {
                tracing::warn!("Threads config was lost during Bluesky login, restoring");
                config.access_token = existing_threads;
            }

            tracing::debug!(
                "Saving config - has_bluesky: {}, has_threads: {}",
                config.has_bluesky(),
                config.has_threads()
            );
            config.save()?;

            println!("Credentials saved to {:?}", Config::path()?);
            println!();
            println!("You can now use ndl with Bluesky!");
            Ok(())
        }
        Err(e) => Err(format!("Authentication failed: {}", e).into()),
    }
}

async fn run_tui() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load()?;

    let mut clients: HashMap<Platform, Box<dyn SocialClient>> = HashMap::new();

    // Initialize Threads if configured
    if config.has_threads() {
        let token = config.access_token.clone().unwrap();
        let client = ThreadsClient::new(token.clone());

        // Verify token is still valid
        match client.get_threads(Some(1)).await {
            Ok(_) => {
                tracing::debug!("Threads token is valid");
                clients.insert(Platform::Threads, Box::new(ThreadsClient::new(token)));
            }
            Err(e) if is_auth_error(&e.to_string()) => {
                tracing::warn!("Threads token expired, skipping");
                eprintln!(
                    "Warning: Threads token expired. Run 'ndl login threads' to re-authenticate."
                );
            }
            Err(e) => {
                tracing::error!("Failed to connect to Threads: {}", e);
                eprintln!("Warning: Failed to connect to Threads: {}", e);
                // Still add the client - TUI will retry
                clients.insert(Platform::Threads, Box::new(ThreadsClient::new(token)));
            }
        }
    }

    // Initialize Bluesky if configured
    if config.has_bluesky() {
        let mut bsky_config = config.bluesky.clone().unwrap();

        // Try to use saved session first
        let client_result = if let Some(ref session) = bsky_config.session {
            tracing::debug!("Attempting to restore Bluesky session");
            match BlueskyClient::from_session(session.clone()).await {
                Ok(client) => {
                    tracing::info!("Successfully restored Bluesky session");
                    Ok(client)
                }
                Err(e) => {
                    tracing::warn!("Failed to restore session, will re-authenticate: {}", e);
                    // Fall back to login
                    BlueskyClient::login(&bsky_config.identifier, &bsky_config.password).await
                }
            }
        } else {
            // No session saved, login normally
            tracing::debug!("No saved session, logging in to Bluesky");
            BlueskyClient::login(&bsky_config.identifier, &bsky_config.password).await
        };

        match client_result {
            Ok(client) => {
                tracing::info!("Successfully connected to Bluesky");

                // Update session in config for next time
                if let Ok(new_session) = client.get_session().await {
                    if bsky_config.session.as_ref() != Some(&new_session) {
                        bsky_config.session = Some(new_session);
                        let mut config_mut = Config::load().unwrap_or_default();
                        config_mut.bluesky = Some(bsky_config);
                        let _ = config_mut.save(); // Best effort, don't fail if this errors
                    }
                }

                clients.insert(Platform::Bluesky, Box::new(client));
            }
            Err(e) => {
                tracing::error!("Failed to connect to Bluesky: {}", e);
                eprintln!("Warning: Failed to connect to Bluesky: {}", e);
                eprintln!("Run 'ndl login bluesky' to update credentials.");
            }
        }
    }

    // Check if we have any platforms configured
    if clients.is_empty() {
        if !config.has_threads() && !config.has_bluesky() {
            eprintln!("No platforms configured. Run one of:");
            eprintln!("  ndl login          - Login to Threads");
            eprintln!("  ndl login bluesky  - Login to Bluesky");
            return Ok(());
        }
        eprintln!("Failed to connect to any platform.");
        return Ok(());
    }

    // Create and run the app
    tracing::info!("Starting TUI with {} platform(s)", clients.len());
    let mut app = App::new(clients);
    app.run().await?;
    tracing::info!("TUI exited");
    Ok(())
}

fn print_usage() {
    println!("Usage: ndl [command]");
    println!();
    println!("Commands:");
    println!("  login [platform]  Authenticate (platforms: threads, bluesky)");
    println!("  logout            Remove saved access token");
    println!("  --version         Show version information");
    println!();
    println!("Examples:");
    println!("  ndl login         - Login to Threads (default)");
    println!("  ndl login bluesky - Login to Bluesky");
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
