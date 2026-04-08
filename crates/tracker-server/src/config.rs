use alloy::{
    network::Ethereum,
    providers::{Provider, RootProvider},
    rpc::client::BuiltInConnectionString,
    transports::{RpcError, TransportErrorKind},
};
use backon::{ExponentialBuilder, Retryable};
use core::fmt::{self, Display, Formatter};
use eyre::Context;
use init4_bin_base::{
    Init4Config,
    utils::{
        from_env::{EnvItemInfo, FromEnv},
        metrics::MetricsConfig,
        provider::ProviderConfig,
        tracing::TracingConfig,
    },
};
use itertools::Itertools;
use signet_constants::SignetSystemConstants;
use signet_tracker::OrderTracker;
use signet_tx_cache::TxCache;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::time::Duration;
use tracing::{debug, info, instrument, warn};
use url::Url;

/// Type alias for the rollup provider (read-only, no fillers needed).
pub type RuProvider = RootProvider<Ethereum>;

/// Type alias for the host chain provider (read-only, no fillers needed).
pub type HostProvider = RootProvider<Ethereum>;

/// Type alias for the concrete [`OrderTracker`] used by the server.
pub type Tracker = OrderTracker<RuProvider, HostProvider>;

const DEFAULT_PORT: u16 = 8019;

/// Configuration for the tracker server, loaded from environment variables.
#[derive(Debug, Clone, FromEnv)]
pub struct Config {
    /// URL for the host chain RPC node.
    #[from_env(var = "HOST_RPC_URL", desc = "URL for Host RPC node")]
    host_rpc: ProviderConfig,

    /// URL for the rollup RPC node.
    #[from_env(var = "ROLLUP_RPC_URL", desc = "URL for Rollup RPC node")]
    ru_rpc: ProviderConfig,

    /// URL of the transaction cache to poll for orders.
    #[from_env(var = "TX_POOL_URL", desc = "URL of the tx pool to poll for orders")]
    tx_pool_url: Url,

    /// Port for the tracker HTTP server.
    #[from_env(
        var = "TRACKER_PORT",
        desc = "Port for the tracker HTTP server [default: 8019]",
        optional
    )]
    port: Option<u16>,

    /// Signet system constants (derived from CHAIN_NAME).
    constants: SignetSystemConstants,

    /// Tracing and OTEL configuration.
    tracing: TracingConfig,

    /// Metrics configuration.
    metrics: MetricsConfig,
}

impl Init4Config for Config {
    fn tracing(&self) -> &TracingConfig {
        &self.tracing
    }

    fn metrics(&self) -> &MetricsConfig {
        &self.metrics
    }
}

/// Builder for [`Config`], allowing programmatic construction without environment variables.
///
/// The three required fields (`host_rpc`, `ru_rpc`, `tx_pool_url`) must be provided. All others
/// have sensible defaults.
#[derive(Debug)]
pub struct ConfigBuilder {
    host_rpc: ProviderConfig,
    ru_rpc: ProviderConfig,
    tx_pool_url: Url,
    port: Option<u16>,
    constants: SignetSystemConstants,
    tracing: TracingConfig,
    metrics: MetricsConfig,
}

impl ConfigBuilder {
    /// Creates a new builder with the required config fields. Uses defaults for the others.
    pub fn new(
        host_rpc: ProviderConfig,
        ru_rpc: ProviderConfig,
        tx_pool_url: Url,
        constants: SignetSystemConstants,
    ) -> Self {
        Self {
            host_rpc,
            ru_rpc,
            tx_pool_url,
            port: None,
            constants,
            tracing: TracingConfig::default(),
            metrics: MetricsConfig::default(),
        }
    }

    /// Sets the HTTP server port.
    pub const fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Sets the tracing configuration.
    pub fn with_tracing(mut self, tracing: TracingConfig) -> Self {
        self.tracing = tracing;
        self
    }

    /// Sets the metrics configuration.
    pub const fn with_metrics(mut self, metrics: MetricsConfig) -> Self {
        self.metrics = metrics;
        self
    }

    /// Builds the [`Config`].
    pub fn build(self) -> Config {
        Config {
            host_rpc: self.host_rpc,
            ru_rpc: self.ru_rpc,
            tx_pool_url: self.tx_pool_url,
            port: self.port,
            constants: self.constants,
            tracing: self.tracing,
            metrics: self.metrics,
        }
    }
}

impl Config {
    /// Port for the tracker HTTP server (defaults to 8019).
    pub const fn port(&self) -> u16 {
        match self.port {
            Some(port) => port,
            None => DEFAULT_PORT,
        }
    }

    /// Signet system constants.
    pub(crate) const fn constants(&self) -> &SignetSystemConstants {
        &self.constants
    }

    /// Connect to the rollup provider with retry.
    pub async fn connect_ru_provider(&self) -> eyre::Result<RuProvider> {
        connect_provider("rollup", &self.ru_rpc).await
    }

    /// Connect to the host chain provider with retry.
    pub async fn connect_host_provider(&self) -> eyre::Result<HostProvider> {
        connect_provider("host", &self.host_rpc).await
    }

    /// Connect to the transaction cache with retry.
    pub async fn connect_tx_cache(&self) -> eyre::Result<TxCache> {
        connect_tx_cache(&self.tx_pool_url).await
    }

    /// Build an [`OrderTracker`] by connecting all providers and the tx-cache.
    pub async fn connect_tracker(&self) -> eyre::Result<Tracker> {
        let (ru_provider, host_provider, tx_cache) = tokio::try_join!(
            self.connect_ru_provider(),
            self.connect_host_provider(),
            self.connect_tx_cache(),
        )?;
        Ok(OrderTracker::new(ru_provider, host_provider, tx_cache, self.constants.clone()))
    }
}

/// Formatted list of environment variables for `--help` output.
pub fn env_var_info() -> String {
    let inventory = Config::inventory();
    let max_width = inventory.iter().map(|item| item.var.len()).max().unwrap_or(0);
    inventory
        .iter()
        .map(|item: &&EnvItemInfo| {
            format!(
                "  {:width$}  {}{}",
                item.var,
                item.description,
                if item.optional { " [optional]" } else { "" },
                width = max_width
            )
        })
        .join("\n")
}

#[instrument(skip_all, fields(provider = %name, url = %DisplayUrl::from(config)))]
async fn connect_provider(
    name: &str,
    config: &ProviderConfig,
) -> eyre::Result<RootProvider<Ethereum>> {
    let attempt = AtomicUsize::new(1);

    let result = (|| async {
        let provider = config.connect().await?;
        provider.get_chain_id().await?;
        Ok(provider)
    })
    .retry(backoff())
    .when(is_transient_transport_error)
    .notify(|error, duration| {
        warn!(
            error = ?error,
            provider = %name,
            attempt = attempt.fetch_add(1, Ordering::Relaxed),
            retry_in_ms = duration.as_millis(),
            "transient error connecting"
        );
    })
    .await
    .wrap_err_with(|| format!("failed to connect to {name} provider"))?;

    info!(provider = %name, "connected");
    Ok(result)
}

#[instrument(skip_all, fields(url = %url))]
async fn connect_tx_cache(url: &url::Url) -> eyre::Result<TxCache> {
    let tx_cache = TxCache::new(url.clone());

    let orders_url = tx_cache
        .url()
        .join("orders")
        .wrap_err("failed to construct transaction cache orders URL")?;

    let attempt = AtomicUsize::new(1);

    let check_connection = || async {
        tx_cache.client().head(orders_url.clone()).send().await?.error_for_status()?;
        Ok(())
    };

    debug!("connecting to transaction cache");

    check_connection
        .retry(backoff())
        .when(is_transient_reqwest_error)
        .notify(|error, duration| {
            warn!(
                error = %error,
                attempt = attempt.fetch_add(1, Ordering::Relaxed),
                retry_in_ms = duration.as_millis(),
                "transient error connecting to transaction cache"
            );
        })
        .await
        .wrap_err("failed to connect to transaction cache")?;

    info!("connected to transaction cache");
    Ok(tx_cache)
}

const fn backoff() -> ExponentialBuilder {
    ExponentialBuilder::new()
        .with_factor(1.5)
        .with_min_delay(Duration::from_millis(100))
        .with_max_delay(Duration::from_secs(10))
        .without_max_times()
}

fn is_transient_transport_error(err: &RpcError<TransportErrorKind>) -> bool {
    match err {
        RpcError::ErrorResp(error) => error.is_retry_err(),
        RpcError::NullResp
        | RpcError::UnsupportedFeature(_)
        | RpcError::LocalUsageError(_)
        | RpcError::SerError(_)
        | RpcError::DeserError { .. } => false,
        RpcError::Transport(error_kind) => error_kind.is_retry_err(),
    }
}

fn is_transient_reqwest_error(err: &reqwest::Error) -> bool {
    if err.is_timeout() || err.is_connect() || err.is_request() {
        return true;
    }
    if let Some(status) = err.status() {
        return status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
    }
    false
}

enum DisplayUrl<'a> {
    Url(&'a str),
    Ipc(std::path::Display<'a>),
    Unknown,
}

impl<'a> Display for DisplayUrl<'a> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DisplayUrl::Url(url) => write!(formatter, "{url}"),
            DisplayUrl::Ipc(path) => write!(formatter, "{path}"),
            DisplayUrl::Unknown => write!(formatter, "unknown"),
        }
    }
}

impl<'a> From<&'a ProviderConfig> for DisplayUrl<'a> {
    fn from(config: &'a ProviderConfig) -> Self {
        match config.connection_string() {
            BuiltInConnectionString::Http(url) | BuiltInConnectionString::Ws(url, _) => {
                DisplayUrl::Url(url.as_str())
            }
            BuiltInConnectionString::Ipc(path) => DisplayUrl::Ipc(path.display()),
            _ => DisplayUrl::Unknown,
        }
    }
}
