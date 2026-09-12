use std::{ops::Deref, process::Stdio, time::Duration};

use alloy::{
    primitives::{Address, U256},
    providers::{DynProvider, Provider},
    signers::local::PrivateKeySigner,
};
use kohaku_userop_kit::bundler::{Bundler, pimlico::PimlicoBundler};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{Child, Command},
    time::timeout,
};

const DEFAULT_PORT: u16 = 3000;
const MAX_START_ATTEMPTS: u16 = 20;
const READY_TIMEOUT: Duration = Duration::from_secs(30);
const READY_LOG_MARKER: &str = "Server listening at";
const PREFUND_ETH: u128 = 1_000 * 10_u128.pow(18);

/// Builds and spawns a local [`alto`](https://github.com/pimlicolabs/alto) ERC-4337
/// bundler process. `alto` must be on `$PATH`.
pub struct AltoBuilder {
    rpc_url: String,
    entrypoint: Address,
    executor_private_key: String,
    utility_private_key: String,
}

/// A running [`alto`](https://github.com/pimlicolabs/alto) process plus a
/// [`Bundler`] client pointed at it.
pub struct AltoInstance {
    child: Child,
    bundler: PimlicoBundler,
}

impl AltoBuilder {
    pub fn new(
        rpc_url: impl Into<String>,
        entrypoint: Address,
        executor_private_key: impl Into<String>,
        utility_private_key: impl Into<String>,
    ) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            entrypoint,
            executor_private_key: executor_private_key.into(),
            utility_private_key: utility_private_key.into(),
        }
    }

    /// Funds the executor/utility addresses with native ETH via `anvil_setBalance`.
    ///
    /// # Errors
    /// Returns an error if a private key fails to parse, or the RPC request fails.
    pub async fn prefund(self, provider: &DynProvider) -> Result<Self, anyhow::Error> {
        for key in [&self.executor_private_key, &self.utility_private_key] {
            let signer: PrivateKeySigner = key.parse()?;
            provider
                .raw_request::<_, ()>(
                    "anvil_setBalance".into(),
                    (signer.address(), U256::from(PREFUND_ETH)),
                )
                .await?;
        }
        Ok(self)
    }

    /// Spawns `alto`, retrying on the next port if a port is already taken.
    ///
    /// # Errors
    /// Returns an error if `alto` fails to start (or never prints its ready marker) on every
    /// attempted port.
    pub async fn spawn(self) -> Result<AltoInstance, anyhow::Error> {
        let mut port = DEFAULT_PORT;
        for attempt in 1..=MAX_START_ATTEMPTS {
            match self.try_spawn(port).await {
                Ok(instance) => return Ok(instance),
                Err(err) if attempt < MAX_START_ATTEMPTS => {
                    tracing::warn!(
                        "alto failed to start on port {port} (attempt {attempt}/{MAX_START_ATTEMPTS}): {err}"
                    );
                    port += 1;
                }
                Err(err) => return Err(err),
            }
        }

        unreachable!("loop above always returns by the last attempt")
    }

    async fn try_spawn(&self, port: u16) -> Result<AltoInstance, anyhow::Error> {
        let mut child = Command::new("alto")
            .arg("--rpc-url")
            .arg(&self.rpc_url)
            .arg("--entrypoints")
            .arg(self.entrypoint.to_string())
            .arg("--executor-private-keys")
            .arg(&self.executor_private_key)
            .arg("--utility-private-key")
            .arg(&self.utility_private_key)
            .arg("--port")
            .arg(port.to_string())
            .arg("--safe-mode")
            .arg("false")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        tokio::spawn(forward_lines(stderr, "alto[stderr]"));

        // Drain stdout for log forwarding.
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(forward_lines_until_marker(
            stdout,
            "alto[stdout]",
            READY_LOG_MARKER,
            ready_tx,
        ));

        match timeout(READY_TIMEOUT, ready_rx).await {
            Ok(Ok(())) => Ok(AltoInstance {
                child,
                bundler: PimlicoBundler::new(format!("http://127.0.0.1:{port}").parse()?),
            }),
            Ok(Err(_)) => {
                let _ = child.start_kill();
                anyhow::bail!("alto exited before printing \"{READY_LOG_MARKER}\"")
            }
            Err(_) => {
                let _ = child.start_kill();
                anyhow::bail!("timed out waiting for alto to start on port {port}")
            }
        }
    }
}

/// Forwards every line from `stdout` to `tracing` for as long as the process runs, firing
/// `ready_tx` the first time a line contains `marker` (alto logs this once its HTTP server has
/// bound).
async fn forward_lines_until_marker(
    stdout: impl AsyncRead + Unpin,
    label: &'static str,
    marker: &str,
    ready_tx: tokio::sync::oneshot::Sender<()>,
) {
    let mut ready_tx = Some(ready_tx);
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::info!("{label}: {line}");
        if let Some(tx) = ready_tx.take_if(|_| line.contains(marker)) {
            let _ = tx.send(());
        }
    }
}

async fn forward_lines(stream: impl AsyncRead + Unpin, label: &'static str) {
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::info!("{label}: {line}");
    }
}

impl Deref for AltoInstance {
    type Target = dyn Bundler;

    fn deref(&self) -> &Self::Target {
        &self.bundler
    }
}

impl Drop for AltoInstance {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}
