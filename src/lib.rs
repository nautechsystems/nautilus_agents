//! Public proposal authoring and assurance SDK for
//! [NautilusTrader](https://nautilustrader.io).
//!
//! The SDK exposes scoped observations, one semantic `ReducePosition` proposal, local policy
//! evaluation, advisory checks, agent-side traces, and transport-neutral public responses. Local
//! results never grant production authority.
//!
//! Status: early alpha. The API and wire contract are not yet stable.

pub mod assurance;
pub mod authoring;
pub mod client;
pub mod protocol;

#[cfg(feature = "conformance")]
pub mod conformance;

#[cfg(feature = "testkit")]
pub mod testing;

/// The crate package version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
