//! The reverse SOCKS5 tunnel to the gateway, run by the embedded wstunnel client.

use std::net::Ipv4Addr;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine;
use hyper::header::HeaderValue;
use tokio::runtime::Runtime;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context as LayerContext, SubscriberExt};
use url::{Host, Url};
use wstunnel::config::{ClientCreationRequest, LocalToRemote};
use wstunnel::executor::DefaultTokioExecutor;
use wstunnel::tunnel::RemoteAddr;
use wstunnel::tunnel::upstream_connectors::Socks5UpstreamConnector;
use wstunnel::{LocalProtocol, create_client};

pub const DEFAULT_GATEWAY: &str = "wss://jaa-net.derpz.org";
// The gateway listens on all its addresses (the fill workers dial its pod IP); its rule remaps 1080 to our own port.
const REMOTE_PORT: u16 = 1080;

/// What the user sees. `Connected` only after the gateway accepted the upgrade.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Connecting,
    Connected,
    /// The last attempt failed, in plain words; the client keeps retrying.
    Retrying(String),
}

/// A running tunnel. Dropping it stops the client and every open connection.
pub struct Tunnel {
    runtime: Option<Runtime>,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

/// Checks the pasted token is "<tenant>:<secret>" and returns it trimmed.
pub fn parse_token(raw: &str) -> Result<&str> {
    let token = raw.trim();
    match token.split_once(':') {
        Some((tenant, secret)) if !tenant.is_empty() && !secret.is_empty() => Ok(token),
        _ => bail!("The token looks incomplete. Paste the whole token, including the part before the colon."),
    }
}

pub fn parse_gateway(raw: &str) -> Result<Url> {
    let url = Url::parse(raw.trim()).context("The gateway address is not a valid URL.")?;
    if !matches!(url.scheme(), "wss" | "ws") || url.host().is_none() {
        bail!("The gateway address must start with wss:// (or ws:// for local testing).");
    }
    Ok(url)
}

pub fn start(gateway: &str, token: &str) -> Result<Tunnel> {
    let token = parse_token(token)?;
    let remote_addr = parse_gateway(gateway)?;
    let credentials = HeaderValue::from_str(&format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(token)
    ))
    .context("The token contains characters that cannot be sent.")?;

    let request = ClientCreationRequest {
        local_to_remote: vec![],
        remote_to_local: vec![LocalToRemote {
            local_protocol: LocalProtocol::ReverseSocks5 { timeout: None, credentials: None },
            local: (Ipv4Addr::UNSPECIFIED, REMOTE_PORT).into(),
            remote: (Host::Domain("localhost".into()), REMOTE_PORT),
        }],
        socket_so_mark: None,
        connection_min_idle: 0,
        connection_retry_max_backoff: Duration::from_secs(300),
        reverse_tunnel_connection_retry_max_backoff: Duration::from_secs(30),
        tls_sni_override: None,
        tls_sni_disable: false,
        tls_ech_enable: false,
        tls_verify_certificate: remote_addr.scheme() == "wss",
        http_proxy: None,
        http_proxy_login: None,
        http_proxy_password: None,
        http_upgrade_path_prefix: "v1".into(),
        http_upgrade_credentials: Some(credentials),
        websocket_ping_frequency: Some(Duration::from_secs(30)),
        websocket_mask_frame: false,
        http_headers: vec![],
        http_headers_file: None,
        remote_addr,
        tls_certificate: None,
        tls_private_key: None,
        dns_resolver: vec![],
        dns_resolver_prefer_ipv4: false,
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("Cannot start the network runtime.")?;
    let executor = DefaultTokioExecutor::new(runtime.handle().clone());
    runtime.spawn(async move {
        let client = match create_client(request, executor).await {
            Ok(client) => client,
            Err(err) => {
                report(Status::Retrying(format!("Cannot set up the connection: {err}")));
                return;
            }
        };
        let remote = RemoteAddr {
            protocol: LocalProtocol::ReverseSocks5 { timeout: None, credentials: None },
            host: Host::Ipv4(Ipv4Addr::UNSPECIFIED),
            port: REMOTE_PORT,
        };
        let cfg = client.config.clone();
        let connector = Socks5UpstreamConnector::new(cfg.socket_so_mark, cfg.timeout_connect, cfg.dns_resolver.clone());
        // Reconnects with backoff and never returns while the runtime lives.
        if let Err(err) = client.run_reverse_tunnel(remote, connector).await {
            report(Status::Retrying(format!("The tunnel stopped: {err}")));
        }
    });
    Ok(Tunnel { runtime: Some(runtime) })
}

static SINK: OnceLock<Box<dyn Fn(Status) + Send + Sync>> = OnceLock::new();

fn report(status: Status) {
    if let Some(sink) = SINK.get() {
        sink(status);
    }
}

/// Installs the log listener that turns wstunnel's own events into `Status` calls.
/// wstunnel exposes no state, so its events are the real signal: a failed attempt
/// logs "Retrying in ...", and a reply from the gateway logs "Server response" or,
/// when no connection came in for 30 s, "No response ... retrying" (the gateway
/// accepted the upgrade and is holding it for traffic).
pub fn watch(sink: impl Fn(Status) + Send + Sync + 'static) {
    let _ = SINK.set(Box::new(sink));
    // Only one global subscriber can be set, and this is the only one.
    let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry().with(StatusLayer));
}

struct StatusLayer;

struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl<S: Subscriber> Layer<S> for StatusLayer {
    fn enabled(&self, metadata: &tracing::Metadata<'_>, _: LayerContext<'_, S>) -> bool {
        metadata.target().starts_with("wstunnel") && *metadata.level() >= Level::DEBUG
    }

    fn on_event(&self, event: &Event<'_>, _: LayerContext<'_, S>) {
        let mut message = Message(String::new());
        event.record(&mut message);
        if let Some(status) = status_of(*event.metadata().level(), &message.0) {
            report(status);
        }
    }
}

fn status_of(level: Level, message: &str) -> Option<Status> {
    if level == Level::ERROR && message.starts_with("Retrying in") {
        return Some(Status::Retrying(plain_error(message)));
    }
    if level == Level::DEBUG
        && (message.starts_with("Server response") || message.starts_with("No response for the reverse tunnel"))
    {
        return Some(Status::Connected);
    }
    None
}

/// Turns wstunnel's failure text into a sentence a user can act on.
fn plain_error(message: &str) -> String {
    if message.contains("status code") || message.contains("Bad Request") || message.contains("Unauthorized") {
        return "The gateway refused the token. Check that you pasted it in full, or ask for a new one.".into();
    }
    let cause = message.split("remote server:").nth(1).unwrap_or(message);
    let first = cause.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("unknown reason");
    format!("Cannot reach the gateway ({first}). Retrying.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_needs_tenant_and_secret() {
        assert_eq!(parse_token("  acme:s3cret\n").unwrap(), "acme:s3cret");
        assert!(parse_token("acme").is_err());
        assert!(parse_token(":s3cret").is_err());
        assert!(parse_token("acme:").is_err());
    }

    #[test]
    fn gateway_must_be_a_websocket_url() {
        assert!(parse_gateway("wss://jaa-net.derpz.org").is_ok());
        assert!(parse_gateway("https://jaa-net.derpz.org").is_err());
        assert!(parse_gateway("jaa-net.derpz.org").is_err());
    }
}
