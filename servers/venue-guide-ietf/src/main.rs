// Copyright 2026 Cisco Systems, Inc. and its affiliates
// SPDX-License-Identifier: Apache-2.0

//! An unofficial guide to an IETF meeting venue, as an MCP server that
//! announces itself on the local network, as specified in
//! draft-jakab-dawn-agent-discovery-mdns-01.

mod agenda;
mod announce;
mod guide;

use std::net::SocketAddr;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::DateTime;
use chrono::Utc;
use clap::Parser;
use rmcp::transport::streamable_http_server::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use crate::agenda::Agenda;
use crate::announce::Announcement;
use crate::guide::Guide;

/// Where the MCP endpoint lives, and the `path` of the announcement.
const MCP_PATH: &str = "/mcp";

/// The newest MCP protocol version the server speaks, announced as
/// `proto-ver`; rmcp's `ProtocolVersion::LATEST`.
const PROTO_VER: &str = "2026-07-28";

const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Parser)]
#[command(version, about)]
struct Args {
    /// IETF meeting number.
    #[arg(long, default_value_t = 127)]
    meeting: u32,

    /// Address to serve MCP on, over Streamable HTTP at /mcp.
    #[arg(long, default_value = "0.0.0.0:8127")]
    listen: SocketAddr,

    /// Use the cached agenda; do not fetch it from the IETF Datatracker.
    #[arg(long)]
    offline: bool,

    /// Where to cache the agenda [default: ~/.cache/venue-guide-ietf].
    #[arg(long)]
    cache_dir: Option<PathBuf>,

    /// Do not announce the guide over mDNS.
    #[arg(long)]
    no_announce: bool,

    /// DNS-SD instance name [default: "IETF <meeting> venue guide"].
    #[arg(long)]
    instance: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("venue_guide_ietf=info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();

    let cache_dir = args
        .cache_dir
        .clone()
        .or_else(default_cache_dir)
        .context("no cache directory: set --cache-dir, or HOME")?;
    let agenda = Arc::new(load_agenda(args.meeting, &cache_dir, args.offline).await?);
    tracing::info!(
        "IETF {} in {}: {} slots",
        agenda.meeting().number,
        agenda.meeting().city,
        agenda.slot_count()
    );

    let host_name = announce::srv_host_name(&gethostname::gethostname().to_string_lossy());
    let stop = CancellationToken::new();
    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(allowed_hosts(&host_name))
        .with_cancellation_token(stop.child_token());
    let service: StreamableHttpService<Guide, LocalSessionManager> = StreamableHttpService::new(
        {
            let agenda = Arc::clone(&agenda);
            move || Ok(Guide::new(Arc::clone(&agenda)))
        },
        Default::default(),
        config,
    );
    let router = axum::Router::new().nest_service(MCP_PATH, service);
    let listener = tokio::net::TcpListener::bind(args.listen)
        .await
        .with_context(|| format!("cannot listen on {}", args.listen))?;
    let port = listener.local_addr()?.port();
    tracing::info!("serving MCP at http://{}{MCP_PATH}", listener.local_addr()?);

    let announcement = if args.no_announce {
        None
    } else {
        let instance = args
            .instance
            .clone()
            .unwrap_or_else(|| format!("IETF {} venue guide", args.meeting));
        let title = format!("Unofficial IETF {} venue guide", args.meeting);
        let txt = announce::txt_record(MCP_PATH, PROTO_VER, &title);
        let announcement = Announcement::start(&instance, &host_name, port, &txt)?;
        // Backticks set the name off from the sentence, since its instance
        // label may contain spaces; inside them, it is the exact DNS name.
        tracing::info!(
            "announced `{}` as http://{}:{port}{MCP_PATH}",
            announcement.fullname(),
            host_name.trim_end_matches('.')
        );
        Some(announcement)
    };

    let server =
        axum::serve(listener, router).with_graceful_shutdown(stop.clone().cancelled_owned());
    let server = tokio::spawn(async move { server.await });

    shutdown_signal().await?;
    tracing::info!("shutting down");
    // Withdraw the announcement first, so browsers drop the guide at once.
    if let Some(announcement) = announcement {
        tokio::task::spawn_blocking(move || announcement.stop()).await?;
        tracing::info!("withdrew the announcement");
    }
    stop.cancel();
    server.await??;
    Ok(())
}

/// Waits for Ctrl-C, or on Unix also for SIGTERM, which `kill`, service
/// managers and task runners send, or SIGHUP, from closing the terminal. Any
/// of them ends with a goodbye; left to their default action, the last two
/// would end the process without one, leaving the announcement in caches for
/// up to its TTL.
async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::SignalKind;
        use tokio::signal::unix::signal;
        let mut terminate = signal(SignalKind::terminate())?;
        let mut hangup = signal(SignalKind::hangup())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
            _ = hangup.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}

/// The `Host` headers to accept: loopback, and the name the guide announces.
/// rmcp rejects any other, against DNS rebinding.
fn allowed_hosts(host_name: &str) -> Vec<String> {
    vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
        host_name.trim_end_matches('.').to_string(),
    ]
}

/// The agenda from the Datatracker, cached for when the network is down; or
/// the cached copy, when fetching fails or `offline` is set.
async fn load_agenda(meeting: u32, cache_dir: &Path, offline: bool) -> anyhow::Result<Agenda> {
    let cache = agenda::cache_file(cache_dir, meeting);
    if !offline {
        match fetch_agenda(meeting).await {
            Ok(json) => {
                let agenda = Agenda::from_json(&json, Utc::now())?;
                if let Err(err) = write_cache(&cache, &json).await {
                    tracing::warn!("cannot cache the agenda at {}: {err:#}", cache.display());
                }
                return Ok(agenda);
            }
            Err(err) => tracing::warn!("cannot fetch the agenda, using the cache: {err:#}"),
        }
    }
    let json = tokio::fs::read(&cache)
        .await
        .with_context(|| format!("no cached agenda at {}", cache.display()))?;
    let fetched_at = tokio::fs::metadata(&cache)
        .await
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(Utc::now);
    tracing::info!("using the agenda cached at {fetched_at}");
    Agenda::from_json(&json, fetched_at)
}

async fn fetch_agenda(meeting: u32) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()?;
    let response = client
        .get(agenda::agenda_url(meeting))
        .send()
        .await?
        .error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

/// Writes the cache through a temporary file, so a crash never leaves half
/// an agenda behind.
async fn write_cache(path: &Path, json: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let partial = path.with_extension("json.partial");
    tokio::fs::write(&partial, json).await?;
    tokio::fs::rename(&partial, path).await?;
    Ok(())
}

fn default_cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .map(|dir| dir.join(env!("CARGO_PKG_NAME")))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn announces_the_newest_protocol_version_rmcp_speaks() {
        let latest = serde_json::to_value(rmcp::model::ProtocolVersion::LATEST).ok();
        assert_eq!(latest, Some(serde_json::Value::from(PROTO_VER)));
    }

    #[test]
    fn accepts_the_announced_host_name() {
        assert_eq!(
            allowed_hosts("venue-guide-mac.local."),
            vec!["localhost", "127.0.0.1", "::1", "venue-guide-mac.local"]
        );
    }
}
