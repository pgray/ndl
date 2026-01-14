mod api;
mod config;
mod oauth;
mod tui;

use api::ThreadsClient;
use config::Config;
use std::env;
use tui::App;

#[tokio::main]
async fn main() {
    // Install rustls crypto provider
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("login") => {
            if let Err(e) = run_login().await {
                eprintln!("Login failed: {}", e);
                std::process::exit(1);
            }
        }
        Some("logout") => {
            if let Err(e) = run_logout() {
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
            // Check auth status
            match Config::load() {
                Ok(config) if config.is_authenticated() => {
                    let token = config.access_token.unwrap();
                    let client = ThreadsClient::new(token);

                    // Fetch initial data
                    let threads = match client.get_threads(Some(25)).await {
                        Ok(resp) => resp.data,
                        Err(e) => {
                            eprintln!("Failed to fetch threads: {}", e);
                            Vec::new()
                        }
                    };

                    // Run TUI
                    let mut app = App::new(client, threads);
                    if let Err(e) = app.run().await {
                        eprintln!("TUI error: {}", e);
                        std::process::exit(1);
                    }
                }
                Ok(_) => {
                    eprintln!("Not logged in. Run `ndl login` to authenticate.");
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("Failed to load config: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

async fn run_login() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::load()?;

    // Get client credentials from config or environment
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

    // Run OAuth flow
    let token = oauth::login(&client_id, &client_secret).await?;

    // Save token to config
    config.access_token = Some(token.access_token);
    config.client_id = Some(client_id);
    config.client_secret = Some(client_secret);
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

fn print_usage() {
    println!("Usage: ndl [command]");
    println!();
    println!("Commands:");
    println!("  login   Authenticate with Threads");
    println!("  logout  Remove saved access token");
    println!();
    println!("Run without arguments to start the TUI.");
}
