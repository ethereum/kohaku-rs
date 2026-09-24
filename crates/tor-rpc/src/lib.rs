//! Alloy JSON-RPC through Arti.
//!
//! Address and state calls use a fresh [`TorClient::isolated_client`] per request.
//! `eth_getLogs` and event-cache downloads reuse the shared Tor client so a long sync
//! does not rebuild a circuit for every window.

use std::{
    io,
    sync::Arc,
    task::{self, Poll},
};

use alloy::{
    providers::RootProvider,
    rpc::{
        client::RpcClient,
        json_rpc::{RequestPacket, ResponsePacket},
    },
    transports::{TransportError, TransportErrorKind, TransportFut},
};
use anyhow::{Context, Result, bail};
use arti_client::{TorClient, TorClientConfig};
use rustls::pki_types::ServerName;
use tokio_rustls::TlsConnector;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tor_rtcompat::PreferredRuntime;
use tower::Service;
use url::Url;

/// A bootstrapped Tor client. Cloning is cheap and shares the directory state.
#[derive(Clone)]
pub struct TorRpc {
    client: TorClient<PreferredRuntime>,
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
        Ok(Self { client })
    }

    /// JSON-RPC provider for `url`.
    ///
    /// Address and state methods get a new circuit. `eth_getLogs` shares the client.
    ///
    /// # Errors
    /// Returns when `url` is a local address an exit cannot reach.
    pub fn rpc_client(&self, url: Url) -> Result<RpcClient> {
        reject_local(&url)?;
        Ok(RpcClient::new(
            TorTransport {
                client: self.client.clone(),
                url,
            },
            false,
        ))
    }

    pub fn provider(&self, url: Url) -> Result<RootProvider> {
        Ok(RootProvider::new(self.rpc_client(url)?))
    }

    /// `GET url` over the shared Tor client (same pool as `eth_getLogs`).
    ///
    /// # Errors
    /// Returns when the download fails or the host is local.
    pub async fn get(&self, url: &Url) -> Result<Vec<u8>> {
        let status = exchange(&self.client, url, "GET", &[], &[], false).await?;
        if !(200..300).contains(&status.code) {
            bail!("GET {url} returned HTTP {}", status.code);
        }
        Ok(status.body)
    }
}

#[derive(Clone)]
struct TorTransport {
    client: TorClient<PreferredRuntime>,
    url: Url,
}

impl Service<RequestPacket> for TorTransport {
    type Response = ResponsePacket;
    type Error = TransportError;
    type Future = TransportFut<'static>;

    fn poll_ready(&mut self, _cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: RequestPacket) -> Self::Future {
        let this = self.clone();
        Box::pin(async move { this.post(req).await })
    }
}

impl TorTransport {
    async fn post(self, req: RequestPacket) -> Result<ResponsePacket, TransportError> {
        let body = serde_json::to_vec(&req).map_err(TransportError::ser_err)?;
        let mut extra = vec![("content-type".to_string(), "application/json".to_string())];
        for (name, value) in req.headers().iter() {
            extra.push((
                name.as_str().to_string(),
                value.to_str().unwrap_or("").to_string(),
            ));
        }
        let isolate = requires_isolation(&req);
        let status = exchange(&self.client, &self.url, "POST", &extra, &body, isolate)
            .await
            .map_err(|err| TransportErrorKind::custom(io::Error::other(err.to_string())))?;
        if !(200..300).contains(&status.code) {
            return Err(TransportErrorKind::http_error(
                status.code,
                String::from_utf8_lossy(&status.body).into_owned(),
            ));
        }
        serde_json::from_slice(&status.body)
            .map_err(|err| TransportError::deser_err(err, String::from_utf8_lossy(&status.body)))
    }
}

struct HttpStatus {
    code: u16,
    body: Vec<u8>,
}

async fn exchange(
    client: &TorClient<PreferredRuntime>,
    url: &Url,
    method: &str,
    headers: &[(String, String)],
    body: &[u8],
    isolate: bool,
) -> Result<HttpStatus> {
    reject_local(url)?;
    let host = url.host_str().context("url host")?.to_string();
    let port = url.port_or_known_default().context("url port")?;
    let https = url.scheme() == "https";
    if url.scheme() != "http" && !https {
        bail!("Tor RPC only supports http and https, got {}", url.scheme());
    }
    match exchange_once(client, &host, port, https, url, method, headers, body, isolate).await {
        Ok(status) => Ok(status),
        Err(_) => {
            exchange_once(client, &host, port, https, url, method, headers, body, isolate).await
        }
    }
}

async fn exchange_once(
    client: &TorClient<PreferredRuntime>,
    host: &str,
    port: u16,
    https: bool,
    url: &Url,
    method: &str,
    headers: &[(String, String)],
    body: &[u8],
    isolate: bool,
) -> Result<HttpStatus> {
    let stream = if isolate {
        client
            .isolated_client()
            .connect((host, port))
            .await
            .with_context(|| format!("Tor connect {host}:{port}"))?
    } else {
        client
            .connect((host, port))
            .await
            .with_context(|| format!("Tor connect {host}:{port}"))?
    };
    let path = match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_string(),
    };
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        path
    };
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAccept: */*\r\n");
    if !body.is_empty() {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    for (name, value) in headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");

    if https {
        let tls = tls_connector();
        let name = ServerName::try_from(host.to_string()).context("tls server name")?;
        let mut stream = tls
            .connect(name, stream.compat())
            .await
            .context("tls through Tor")?;
        write_and_read(&mut stream, request.as_bytes(), body).await
    } else {
        let mut stream = stream.compat();
        write_and_read(&mut stream, request.as_bytes(), body).await
    }
}

fn tls_connector() -> TlsConnector {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
}

async fn write_and_read(
    stream: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
    head: &[u8],
    body: &[u8],
) -> Result<HttpStatus> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream.write_all(head).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await?;
    parse_http(&raw)
}

fn parse_http(raw: &[u8]) -> Result<HttpStatus> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .context("http headers")?;
    let header = std::str::from_utf8(&raw[..split]).context("http header utf8")?;
    let mut lines = header.split("\r\n");
    let status = lines.next().context("http status")?;
    let code: u16 = status
        .split_whitespace()
        .nth(1)
        .context("http status code")?
        .parse()
        .context("http status code")?;
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            length = value.trim().parse().ok();
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
    }
    let rest = &raw[split + 4..];
    let body = if chunked {
        decode_chunks(rest)?
    } else if let Some(n) = length {
        rest.get(..n).context("short http body")?.to_vec()
    } else {
        rest.to_vec()
    };
    Ok(HttpStatus { code, body })
}

fn decode_chunks(mut rest: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = rest
            .windows(2)
            .position(|w| w == b"\r\n")
            .context("chunk size")?;
        let size = std::str::from_utf8(&rest[..line_end]).context("chunk size")?;
        let size = usize::from_str_radix(size.trim().split(';').next().unwrap_or("0"), 16)
            .context("chunk size")?;
        rest = &rest[line_end + 2..];
        if size == 0 {
            break;
        }
        if rest.len() < size + 2 {
            bail!("short chunk");
        }
        out.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
    Ok(out)
}

fn requires_isolation(req: &RequestPacket) -> bool {
    !req.method_names().all(|method| method == "eth_getLogs")
}

fn reject_local(url: &Url) -> Result<()> {
    let host = url.host_str().unwrap_or("");
    let local = host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host.ends_with(".local")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || host.starts_with("172.16.")
        || host.is_empty();
    if local {
        bail!("{host} is not reachable through a Tor exit");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localhost_is_rejected() {
        let url: Url = "http://127.0.0.1:8545".parse().unwrap();
        assert!(reject_local(&url).is_err());
    }

    #[test]
    fn parses_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhelloTRAIL";
        let status = parse_http(raw).unwrap();
        assert_eq!(status.code, 200);
        assert_eq!(status.body, b"hello");
    }

    #[tokio::test]
    #[ignore = "bootstraps Tor and times three isolated JSON-RPC calls"]
    async fn isolated_request_latency() {
        let tor = TorRpc::connect().await.unwrap();
        let url: Url = "https://ethereum.publicnode.com".parse().unwrap();
        let provider = tor.provider(url).unwrap();
        let started = std::time::Instant::now();
        let head = alloy::providers::Provider::get_block_number(&provider)
            .await
            .unwrap();
        eprintln!(
            "isolated eth_blockNumber -> {head} in {:.1}s",
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
            match alloy::providers::Provider::get_logs(&provider, &filter).await {
                Ok(logs) => eprintln!(
                    "isolated eth_getLogs {i} blocks {from}..={head} {} logs in {:.1}s",
                    logs.len(),
                    started.elapsed().as_secs_f64()
                ),
                Err(err) => eprintln!(
                    "isolated eth_getLogs {i} blocks {from}..={head} failed in {:.1}s: {err}",
                    started.elapsed().as_secs_f64()
                ),
            }
        }
    }
}
