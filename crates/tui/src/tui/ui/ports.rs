//! UI-layer IO adapters for the `BalanceProvider` port (dependency inversion).
//!
//! The port trait itself lives in `mimofan_core` so any crate can depend on
//! the abstraction. This module supplies the concrete `reqwest`-backed
//! adapter used by the TUI.

use std::future::Future;
use std::time::Duration;

use mimofan_core::BalanceProvider;

use crate::pricing::{BalanceInfo, BalanceResponse};

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
    type Balance = BalanceInfo;

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
