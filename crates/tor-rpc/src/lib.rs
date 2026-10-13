//! Alloy JSON-RPC through Arti (Tor network)
//!
//! [`TorRpc::connect`] bootstraps one client. Cloning [`TorRpc`] shares it.
//! Requests on one [`TorClient`] handle may share circuits with each other.
//! A new [`TorClient::isolated_client`] is a new group: its streams do not share
//! circuits with the parent or with any other isolated client. It is not one
//! circuit per RPC. Call `isolated_client` again for the next unlinkable group.
//!
//! - [`TorRpc::shared_rpc_client`], [`TorRpc::shared_provider`], [`TorRpc::send_shared`],
//!   [`TorRpc::get`], [`TorRpc::post`] — the bootstrapped client. Keep one for traffic that is safe
//!   to link: `eth_blockNumber`, generic/global `eth_getLogs` , e.g a PIR HTTP POST basically any
//!   request that does not contain privacy-sensitive data e.g. specific user accounts
//! - [`TorRpc::isolated_rpc_client`], [`TorRpc::isolated_provider`] — one fresh `isolated_client`,
//!   reused for every RPC on that client. Hold it for a session (the calls about one address).
//!   Construct another when the next session must not share circuits with it.
//! - [`TorRpc::with_isolated`] — that same client for one closure, then drop it.
//! - [`TorRpc::send_isolated`] — one JSON-RPC packet on a new `isolated_client`, then drop it.
//!   Calls packed into that packet share the group. The next call is a new group.

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use alloy::{
    providers::RootProvider,
    rpc::{
        client::RpcClient,
        json_rpc::{RequestPacket, ResponsePacket},
    },
    transports::{
        TransportError,
        http::{
            Http, HyperClient,
            hyper::{self, body::Bytes},
            hyper_util::{
                self,
                client::legacy::connect::{Connected, Connection},
                rt::{TokioExecutor, TokioIo},
            },
        },
    },
};
use anyhow::{Context as _, Result, bail};
use arti_client::{DataStream, TorClient, TorClientConfig};
use http_body_util::{BodyExt, Full};
use hyper_rustls::HttpsConnectorBuilder;
use tor_rtcompat::PreferredRuntime;
use tower::Service;
use url::Url;

/// Raw HTTP response from a Tor exit request (shared client).
#[derive(Clone, Debug)]
pub struct TorHttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response header lines (name, value).
    pub headers: Vec<(String, String)>,
    /// Response body.
    pub body: Vec<u8>,
}

impl TorHttpResponse {
    /// First header value matching `name` (case-insensitive).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A bootstrapped Tor client. Cloning is cheap and shares the directory state
/// and the shared HTTP connection pool.
///
/// Arti refuses local addresses by default. That error happens when a request
/// is sent, not when one of these clients is built.
#[derive(Clone)]
pub struct TorRpc {
    client: TorClient<PreferredRuntime>,
    http: Pool,
}

impl TorRpc {
    /// Bootstrap a Tor client. Fails if the local network cannot reach the Tor directory.
    ///
    /// # Errors
    /// Returns when Arti cannot bootstrap.
    pub async fn connect() -> Result<Self> {
        let client = TorClient::create_bootstrapped(TorClientConfig::default())
            .await
            .context("bootstrap Tor")?;
        // `ClientConfig::builder` panics unless a process-wide provider is installed.
        // Arti may already have installed ring; a second attempt returns `Err`.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let http = pool(client.clone());
        Ok(Self { client, http })
    }

    /// Long-lived JSON-RPC client that reuses the shared Tor client (and its circuits).
    #[must_use]
    pub fn shared_rpc_client(&self, url: Url) -> RpcClient {
        RpcClient::new(transport(self.http.clone(), url), false)
    }

    /// JSON-RPC client pinned to a fresh [`TorClient::isolated_client`].
    ///
    /// All requests on this client share one isolation token / circuit family.
    #[must_use]
    pub fn isolated_rpc_client(&self, url: Url) -> RpcClient {
        RpcClient::new(transport(pool(self.client.isolated_client()), url), false)
    }

    /// Run `f` with an [`isolated_rpc_client`](Self::isolated_rpc_client), then drop it.
    ///
    /// # Errors
    /// Returns when `f` fails.
    pub async fn with_isolated<F, Fut, T>(&self, url: Url, f: F) -> Result<T>
    where
        F: FnOnce(RpcClient) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        f(self.isolated_rpc_client(url)).await
    }

    /// Shared-circuit Alloy provider (pool sync).
    #[must_use]
    pub fn shared_provider(&self, url: Url) -> RootProvider {
        RootProvider::new(self.shared_rpc_client(url))
    }

    /// Isolated-session Alloy provider.
    #[must_use]
    pub fn isolated_provider(&self, url: Url) -> RootProvider {
        RootProvider::new(self.isolated_rpc_client(url))
    }

    /// `GET url` over the shared Tor client.
    ///
    /// # Errors
    /// Returns when the download fails or the status is not 2xx.
    pub async fn get(&self, url: &Url) -> Result<Vec<u8>> {
        self.request_shared(url, "GET", &[], &[]).await
    }

    /// Arbitrary `POST` request with `body` over the shared Tor client.
    ///
    /// Used by e.g. PIR that must reach a PIR HTTP API without clearnet.
    ///
    /// # Errors
    /// Returns when the upload fails or the status is not 2xx.
    pub async fn post(
        &self,
        url: &Url,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<Vec<u8>> {
        self.request_shared(url, "POST", headers, body).await
    }

    /// Arbitrary HTTP request over the shared Tor client.
    ///
    /// # Errors
    /// Returns when the exchange fails or the status is not 2xx.
    pub async fn request_shared(
        &self,
        url: &Url,
        method: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<Vec<u8>> {
        let resp = self.http_shared(url, method, headers, body).await?;
        if !(200..300).contains(&resp.status) {
            bail!("{method} {url} returned HTTP {}", resp.status);
        }
        Ok(resp.body)
    }

    /// HTTP over the shared Tor client, including response headers.
    ///
    /// # Errors
    /// Returns when Tor or the HTTP exchange fails (not on non-2xx status).
    pub async fn http_shared(
        &self,
        url: &Url,
        method: &str,
        headers: &[(String, String)],
        body: &[u8],
    ) -> Result<TorHttpResponse> {
        exchange(&self.http, url, method, headers, body).await
    }

    /// Send a JSON-RPC packet over the shared Tor client (one HTTP POST).
    ///
    /// # Errors
    /// Returns a transport error when serialization, Tor, or HTTP fails.
    pub async fn send_shared(
        &self,
        url: Url,
        packet: RequestPacket,
    ) -> Result<ResponsePacket, TransportError> {
        Service::call(&mut transport(self.http.clone(), url), packet).await
    }

    /// Send a JSON-RPC packet on a fresh [`TorClient::isolated_client`].
    ///
    /// Batch all RPCs that should share one isolation token into `packet`.
    ///
    /// # Errors
    /// Returns a transport error when serialization, Tor, or HTTP fails.
    pub async fn send_isolated(
        &self,
        url: Url,
        packet: RequestPacket,
    ) -> Result<ResponsePacket, TransportError> {
        Service::call(
            &mut transport(pool(self.client.isolated_client()), url),
            packet,
        )
        .await
    }
}

type Pool =
    hyper_util::client::legacy::Client<hyper_rustls::HttpsConnector<TorConnector>, Full<Bytes>>;

fn pool(client: TorClient<PreferredRuntime>) -> Pool {
    let https = HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_or_http()
        .enable_http1()
        .wrap_connector(TorConnector(client));
    hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(https)
}

fn transport(pool: Pool, url: Url) -> Http<HyperClient<Full<Bytes>, Pool>> {
    Http::with_client(HyperClient::with_service(pool), url)
}

async fn exchange(
    http: &Pool,
    url: &Url,
    method: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<hyper::Response<Bytes>> {
    let mut builder = hyper::Request::builder().method(method).uri(url.as_str());
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    let request = builder
        .body(Full::new(Bytes::copy_from_slice(body)))
        .context("build http request")?;
    let mut client = http.clone();
    let response = Service::call(&mut client, request)
        .await
        .context("http through Tor")?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    let body = response
        .into_body()
        .collect()
        .await
        .context("http body")?
        .to_bytes()
        .to_vec();
    Ok(TorHttpResponse {
        status,
        headers,
        body,
    })
}

/// Dial `(host, port)` through Arti. hyper-rustls wraps the stream in TLS for `https`.
#[derive(Clone)]
struct TorConnector(TorClient<PreferredRuntime>);

type TorConnectFut =
    Pin<Box<dyn Future<Output = Result<TokioIo<TorStream>, anyhow::Error>> + Send>>;

impl Service<hyper::Uri> for TorConnector {
    type Response = TokioIo<TorStream>;
    type Error = anyhow::Error;
    type Future = TorConnectFut;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: hyper::Uri) -> Self::Future {
        let client = self.0.clone();
        Box::pin(async move {
            let host = uri.host().context("url host")?.to_owned();
            let port = uri
                .port_u16()
                .unwrap_or(if uri.scheme_str() == Some("https") {
                    443
                } else {
                    80
                });
            let stream = client
                .connect((host.as_str(), port))
                .await
                .with_context(|| format!("Tor connect {host}:{port}"))?;
            Ok(TokioIo::new(TorStream(stream)))
        })
    }
}

/// `DataStream` does not implement hyper's [`Connection`](Connection) metadata trait.
struct TorStream(DataStream);

impl Connection for TorStream {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}

impl tokio::io::AsyncRead for TorStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for TorStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "bootstraps Tor and times shared vs isolated JSON-RPC calls"]
    async fn circuit_session_latency() {
        let tor = TorRpc::connect().await.unwrap();
        let url: Url = "https://ethereum.publicnode.com".parse().unwrap();
        let shared = tor.shared_provider(url.clone());
        let started = std::time::Instant::now();
        let head = alloy::providers::Provider::get_block_number(&shared)
            .await
            .unwrap();
        eprintln!(
            "shared eth_blockNumber -> {head} in {:.1}s",
            started.elapsed().as_secs_f64()
        );
        let from = head.saturating_sub(31);
        let weth: alloy::primitives::Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
            .parse()
            .unwrap();
        let filter = alloy::rpc::types::Filter::new()
            .address(weth)
            .from_block(from)
            .to_block(head);
        for i in 0..2 {
            let started = std::time::Instant::now();
            match alloy::providers::Provider::get_logs(&shared, &filter).await {
                Ok(logs) => eprintln!(
                    "shared eth_getLogs {i} blocks {from}..={head} {} logs in {:.1}s",
                    logs.len(),
                    started.elapsed().as_secs_f64()
                ),
                Err(err) => eprintln!(
                    "shared eth_getLogs {i} blocks {from}..={head} failed in {:.1}s: {err}",
                    started.elapsed().as_secs_f64()
                ),
            }
        }
        let started = std::time::Instant::now();
        let n = tor
            .with_isolated(url, |client| async move {
                let p: RootProvider = RootProvider::new(client);
                Ok(alloy::providers::Provider::get_block_number(&p).await?)
            })
            .await
            .unwrap();
        eprintln!(
            "isolated eth_blockNumber -> {n} in {:.1}s",
            started.elapsed().as_secs_f64()
        );
    }
}
