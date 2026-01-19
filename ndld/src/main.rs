use ndld::auth::{OAuthConfig, SessionStore, spawn_cleanup_task};
use ndld::routes::{AppState, create_router};

use axum_server::tls_rustls::RustlsConfig;
use axum_server::Handle;
use rustls_acme::caches::DirCache;
use rustls_acme::AcmeConfig;
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tokio_stream::StreamExt;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const LETS_ENCRYPT_PRODUCTION: &str = "https://acme-v02.api.letsencrypt.org/directory";
const LETS_ENCRYPT_STAGING: &str = "https://acme-staging-v02.api.letsencrypt.org/directory";

/// Graceful shutdown signal handler for SIGTERM and SIGINT
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received SIGINT, starting graceful shutdown");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM, starting graceful shutdown");
        }
    }
}

/// Spawn a task that triggers graceful shutdown via Handle
fn spawn_shutdown_handler(handle: Handle<SocketAddr>) {
    tokio::spawn(async move {
        shutdown_signal().await;
        handle.graceful_shutdown(Some(std::time::Duration::from_secs(30)));
    });
}

fn print_version() {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const GIT_VERSION: &str = env!("NDLD_GIT_VERSION");
    println!("ndld {} ({})", VERSION, GIT_VERSION);
}

#[tokio::main]
async fn main() {
    // Handle --version flag before anything else
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("--version")
        || args.get(1).map(|s| s.as_str()) == Some("-V")
    {
        print_version();
        return;
    }

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ndld=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Install rustls crypto provider
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Load configuration from environment
    let client_id = env::var("NDL_CLIENT_ID").expect("NDL_CLIENT_ID must be set");
    let client_secret = env::var("NDL_CLIENT_SECRET").expect("NDL_CLIENT_SECRET must be set");
    let public_url = env::var("NDLD_PUBLIC_URL").expect("NDLD_PUBLIC_URL must be set");
    let port: u16 = env::var("NDLD_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    // TLS options (priority: ACME > manual TLS > plain HTTP)
    let acme_domain = env::var("NDLD_ACME_DOMAIN").ok();
    let acme_email = env::var("NDLD_ACME_EMAIL").ok();
    let acme_dir = env::var("NDLD_ACME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/ndld/acme"));
    let acme_staging = env::var("NDLD_ACME_STAGING").is_ok();

    let tls_cert = env::var("NDLD_TLS_CERT").ok();
    let tls_key = env::var("NDLD_TLS_KEY").ok();

    let oauth = OAuthConfig {
        client_id,
        client_secret,
        public_url,
    };

    let sessions = SessionStore::new();

    // Spawn cleanup task
    spawn_cleanup_task(sessions.clone());

    let state = Arc::new(AppState { sessions, oauth });

    let app = create_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    // Priority: ACME > manual TLS > plain HTTP
    if let Some(domain) = acme_domain {
        let email = acme_email.expect("NDLD_ACME_EMAIL must be set when using ACME");
        let directory_url = if acme_staging {
            tracing::warn!("Using Let's Encrypt STAGING environment");
            LETS_ENCRYPT_STAGING
        } else {
            LETS_ENCRYPT_PRODUCTION
        };

        tracing::info!(
            "Starting ndld server with ACME/Let's Encrypt on {} for {}",
            addr,
            domain
        );
        tracing::info!("ACME cache directory: {:?}", acme_dir);

        // Ensure cache directory exists
        std::fs::create_dir_all(&acme_dir).expect("Failed to create ACME cache directory");

        let mut acme_state = AcmeConfig::new([domain])
            .contact_push(format!("mailto:{}", email))
            .cache(DirCache::new(acme_dir))
            .directory(directory_url)
            .state();

        let acceptor = acme_state.axum_acceptor(acme_state.default_rustls_config());

        // Spawn task to log ACME events
        tokio::spawn(async move {
            loop {
                match acme_state.next().await {
                    Some(Ok(ok)) => tracing::info!("ACME event: {:?}", ok),
                    Some(Err(err)) => tracing::error!("ACME error: {:?}", err),
                    None => break,
                }
            }
        });

        let handle = Handle::new();
        spawn_shutdown_handler(handle.clone());

        axum_server::bind(addr)
            .handle(handle)
            .acceptor(acceptor)
            .serve(app.into_make_service())
            .await
            .expect("Server error");

        tracing::info!("Server shutdown complete");
    } else {
        match (tls_cert, tls_key) {
            (Some(cert_path), Some(key_path)) => {
                tracing::info!("Starting ndld server with TLS on {}", addr);
                let config = RustlsConfig::from_pem_file(&cert_path, &key_path)
                    .await
                    .expect("Failed to load TLS certificate");

                let handle = Handle::new();
                spawn_shutdown_handler(handle.clone());

                axum_server::bind_rustls(addr, config)
                    .handle(handle)
                    .serve(app.into_make_service())
                    .await
                    .expect("Server error");

                tracing::info!("Server shutdown complete");
            }
            (None, None) => {
                tracing::info!("Starting ndld server on {}", addr);
                let listener = tokio::net::TcpListener::bind(addr)
                    .await
                    .expect("Failed to bind to address");

                axum::serve(listener, app)
                    .with_graceful_shutdown(shutdown_signal())
                    .await
                    .expect("Server error");

                tracing::info!("Server shutdown complete");
            }
            _ => {
                panic!("Both NDLD_TLS_CERT and NDLD_TLS_KEY must be set for TLS, or neither");
            }
        }
    }
}
