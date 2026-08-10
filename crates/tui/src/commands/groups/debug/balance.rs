//! Balance: query the active provider's account balance or credit status.
//!
//! The balance is fetched by background tasks in the UI event loop (once at
//! startup, then debounced on turn completion) and deposited into
//! `app.balance_cell`. This command reads that same cell, so `/balance` and
//! the footer chip can never disagree.

use crate::config::ApiProvider;
use crate::pricing::BalanceInfo;
use crate::tui::app::App;

use super::CommandResult;

/// Render the currency symbol the footer uses for this balance.
fn currency_symbol(currency: &str) -> &'static str {
    match currency {
        "CNY" | "cny" => "¥",
        _ => "$",
    }
}

/// Format one balance line, skipping components the provider left empty.
/// `total_balance` is always shown; the granted/topped-up split is only
/// meaningful when the provider actually reports it.
fn format_balance(info: &BalanceInfo) -> String {
    let symbol = currency_symbol(&info.currency);
    let mut text = format!(
        "**Total**: {symbol}{} {}\n",
        info.total_balance, info.currency
    );
    if !info.granted_balance.is_empty() {
        text.push_str(&format!("**Granted**: {symbol}{}\n", info.granted_balance));
    }
    if !info.topped_up_balance.is_empty() {
        text.push_str(&format!(
            "**Topped up**: {symbol}{}\n",
            info.topped_up_balance
        ));
    }
    text
}

/// Query provider account balance / credits.
pub fn balance(app: &mut App) -> CommandResult {
    let provider = app.api_provider;

    // Only the OpenAI-compatible (DeepSeek) endpoint exposes a balance API;
    // the background fetch is gated on the same condition, so for any other
    // provider the cell is guaranteed empty and "unsupported" is the honest
    // answer rather than "not fetched yet".
    if !matches!(provider, ApiProvider::OpenAiCompatible) {
        return CommandResult::message(format!(
            "Balance lookup is not supported for {}. Check the provider dashboard for account balance details.",
            provider.display_name()
        ));
    }

    let cached = match app.balance_cell.lock() {
        Ok(guard) => guard.clone(),
        // The cell is only ever held for a clone, so poisoning means another
        // thread panicked mid-update. Report it instead of panicking again.
        Err(_) => {
            return CommandResult::message(
                "Balance is temporarily unavailable: the balance cache was left in an inconsistent state.".to_string(),
            );
        }
    };

    match cached {
        Some(info) => {
            let mut text = format!("## Balance — {}\n\n", provider.display_name());
            text.push_str(&format_balance(&info));
            CommandResult::message(text)
        }
        // Reachable when the initial fetch has not completed, the API key is
        // missing, or the request failed. The fetch is retried on turn
        // completion, so this is not a permanent state.
        None => CommandResult::message(format!(
            "Balance for {} has not been retrieved yet. It is fetched in the background at startup and refreshed after each turn — retry shortly, and verify the API key if it keeps showing as unavailable.",
            provider.display_name()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(currency: &str, total: &str, granted: &str, topped_up: &str) -> BalanceInfo {
        BalanceInfo {
            currency: currency.to_string(),
            total_balance: total.to_string(),
            granted_balance: granted.to_string(),
            topped_up_balance: topped_up.to_string(),
        }
    }

    #[test]
    fn cny_renders_with_yuan_sign_matching_the_footer() {
        let text = format_balance(&info("CNY", "42.50", "", ""));
        assert!(text.contains("¥42.50"), "{text}");
        assert!(text.contains("CNY"), "{text}");
    }

    #[test]
    fn unknown_currency_falls_back_to_dollar_like_the_footer() {
        let text = format_balance(&info("USD", "7.00", "", ""));
        assert!(text.contains("$7.00"), "{text}");
    }

    #[test]
    fn empty_components_are_omitted_rather_than_shown_as_blank() {
        let text = format_balance(&info("CNY", "10", "", ""));
        assert!(!text.contains("Granted"), "{text}");
        assert!(!text.contains("Topped up"), "{text}");
    }

    #[test]
    fn reported_components_are_all_shown() {
        let text = format_balance(&info("CNY", "10", "4", "6"));
        assert!(text.contains("Granted"), "{text}");
        assert!(text.contains("Topped up"), "{text}");
    }
}
