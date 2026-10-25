use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use tokio::net::TcpListener;

use leash_proxy::cli::Cli;
use leash_proxy::config::ProxyConfig;
use leash_proxy::logging;
use leash_proxy::policy::{CedarEngine, EngineStack, PolicyEngine};
use leash_proxy::proxy::{ProxyState, StaticAgentResolver, serve};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let config = match ProxyConfig::load(&cli) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("leash-proxy: configuration error: {err}");
            return ExitCode::FAILURE;
        }
    };

    // Keep alive for the process lifetime: flushes the background log
    // writer.
    let _log_guard = match logging::init(config.log_file.as_deref()) {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("leash-proxy: failed to initialize logging: {err}");
            return ExitCode::FAILURE;
        }
    };

    tracing::info!(
        target: "leash_proxy",
        version = env!("CARGO_PKG_VERSION"),
        listen = %config.listen,
        "starting leash-proxy"
    );

    let cedar = Arc::new(CedarEngine::from_files(&config.schema, &config.policies));
    if !cedar.is_ready() {
        tracing::error!(
            target: "leash_proxy",
            "cedar engine is in fail-closed mode; every request will be denied until policies load"
        );
    }
    let engine: Arc<dyn PolicyEngine> = Arc::new(EngineStack::new(vec![cedar]));
    let resolver = Arc::new(StaticAgentResolver::from_entries(&config.agents));

    let listener = match TcpListener::bind(&config.listen).await {
        Ok(listener) => listener,
        Err(err) => {
            tracing::error!(target: "leash_proxy", listen = %config.listen, %err, "failed to bind listener");
            return ExitCode::FAILURE;
        }
    };

    let state = Arc::new(ProxyState::new(
        engine,
        resolver,
        config.header_allowlist.clone(),
    ));
    tracing::info!(
        target: "leash_proxy",
        listen = %config.listen,
        policy_engine = "cedar",
        policy_files = config.policies.len(),
        "leash-proxy ready; waiting for agent traffic"
    );

    tokio::select! {
        result = serve(listener, state) => {
            if let Err(err) = result {
                tracing::error!(target: "leash_proxy", %err, "proxy server terminated");
                return ExitCode::FAILURE;
            }
        }
        _ = shutdown_signal() => {
            tracing::info!(target: "leash_proxy", "shutdown signal received; stopping");
        }
    }

    ExitCode::SUCCESS
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = &mut ctrl_c => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl-c handler");
    }
}
