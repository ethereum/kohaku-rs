//! Hybrid Ethereum JSON-RPC provider: PIR for private account reads, fallback
//! RPC for everything else.

#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

mod error;
mod fallback;
mod lookup;
mod router;
mod routes;
mod transport;

pub use error::PirProviderError;
pub use fallback::{FallbackRpc, HttpFallback, MapFallback};
#[cfg(feature = "client")]
pub use lookup::PirLookup;
pub use lookup::{LookupBackend, MapLookup};
pub use pir_keyword::manifest::DatasetManifest;
pub use router::{PirProviderConfig, PirRouter};
pub use routes::{CallMatch, Route, RouteTable};
#[cfg(feature = "client")]
pub use transport::connect_provider;
pub use transport::{PirConnect, PirTransport};
