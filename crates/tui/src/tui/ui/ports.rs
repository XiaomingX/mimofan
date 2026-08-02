//! UI-layer IO ports (dependency inversion).
//!
//! The TUI previously reached for concrete IO (`reqwest`, `std::fs`,
//! `std::process`) directly at call sites. These traits capture the *intent*
//! behind that IO so the UI depends on an abstraction; concrete adapters are
//! supplied at the boundary. As this refactor matures, port traits like these
//! are natural candidates to be promoted into the `core` crate (task B).

use std::future::Future;
use std::time::Duration;

use crate::pricing::{BalanceInfo, BalanceResponse};

/// Port for fetching the account balance from the provider's balance API.
///
/// Abstracts the HTTP call so the balance UI no longer depends on a concrete
/// `reqwest` client. Adapters implement this against real or fake backends.
///
/// The returned future is `Send`; the balance refresh paths spawn background
/// tasks, so the trait contract must guarantee `Send` (RPITIT with `+ Send`).
pub(crate) trait BalanceProvider: Send + Sync {
    fn fetch_balance(
        &self,
        api_key: &str,
        base_url: &str,
    ) -> impl Future<Output = Option<BalanceInfo>> + Send;
}

/// Concrete `reqwest`-backed adapter.
///
/// Encapsulates the shared, pooled HTTP client that used to live as a
/// module-level `static BALANCE_CLIENT` in `ui/mod.rs`. Cheap to `Arc`-clone,
/// so a single pooled client can be shared into the background refresh tasks.
pub(crate) struct ReqwestBalanceProvider {
    client: ::reqwest::Client,
}

impl ReqwestBalanceProvider {
    pub(crate) fn new() -> Self {
        let client = crate::tls::reqwest_client_builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self { client }
    }
}

impl BalanceProvider for ReqwestBalanceProvider {
    fn fetch_balance(
        &self,
        api_key: &str,
        base_url: &str,
    ) -> impl Future<Output = Option<BalanceInfo>> + Send {
        async move {
            let url = format!("{}/user/balance", base_url.trim_end_matches('/'));
            let response = self
                .client
                .get(url)
                .header("Authorization", format!("Bearer {api_key}"))
                .send()
                .await
                .ok()?;
            if !response.status().is_success() {
                tracing::debug!(
                    "balance API returned {}: {}",
                    response.status().as_u16(),
                    response.text().await.unwrap_or_default()
                );
                return None;
            }
            let body: BalanceResponse = response.json().await.ok()?;
            // Return the first balance entry (typically the user's primary currency).
            body.balance_infos.into_iter().next()
        }
    }
}
