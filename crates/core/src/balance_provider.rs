//! Runtime IO ports (dependency inversion).
//!
//! Port traits capture the *intent* behind concrete IO (HTTP, fs, process)
//! so higher layers depend on an abstraction instead of a concrete client.
//! Concrete adapters are supplied at the boundary (e.g. the `tui` crate's
//! `ReqwestBalanceProvider`).
//!
//! `BalanceProvider` uses an associated `Balance` type so this port can live
//! in the bottom `core` crate without depending on any higher-layer type.

use std::future::Future;

/// Port for fetching the account balance from the provider's balance API.
///
/// Abstracts the HTTP call so the balance UI (or any consumer) no longer
/// depends on a concrete `reqwest` client. Adapters implement this against
/// real or fake backends.
///
/// The returned future is `Send`; balance-refresh paths spawn background
/// tasks, so the trait contract must guarantee `Send` (RPITIT with `+ Send`).
pub trait BalanceProvider: Send + Sync {
    /// Concrete balance type produced by the adapter.
    type Balance;

    fn fetch_balance(
        &self,
        api_key: &str,
        base_url: &str,
    ) -> impl Future<Output = Option<Self::Balance>> + Send;
}
