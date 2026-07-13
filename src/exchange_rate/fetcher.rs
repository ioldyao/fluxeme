use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use crate::db::Database;

const FRANKFURTER_API: &str = "https://api.frankfurter.dev/v1";

/// Response from Frankfurter API `/latest?from=USD`
#[derive(Debug, Deserialize)]
struct FrankfurterResponse {
    #[allow(dead_code)]
    amount: f64,
    base: String,
    date: String,
    rates: std::collections::HashMap<String, f64>,
}

/// Fetch latest exchange rates from Frankfurter API and store them in the DB.
///
/// Called on startup and periodically by the background task, or manually
/// via the admin API refresh endpoint.
pub async fn fetch_and_store_rates(
    db: &Arc<Database>,
    quote_currencies: &[&str],
) -> Result<usize, String> {
    let url = format!("{}/latest?from=USD", FRANKFURTER_API);
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Frankfurter request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Frankfurter returned HTTP {}", resp.status()));
    }

    let data: FrankfurterResponse = resp
        .json()
        .await
        .map_err(|e| format!("Frankfurter parse failed: {}", e))?;

    let mut count = 0usize;
    for currency in quote_currencies {
        let upper = currency.to_uppercase();
        if upper == data.base {
            continue;
        }
        if let Some(&rate) = data.rates.get(&upper) {
            db.upsert_exchange_rate(
                &data.base,
                &upper,
                rate,
                &data.date,
                "frankfurter",
                None,
            )
            .await
            .map_err(|e| format!("DB upsert failed for {}: {}", upper, e))?;
            count += 1;
            tracing::info!(from = %data.base, to = %upper, rate, date = %data.date, "Exchange rate stored");
        } else {
            tracing::warn!(currency = %upper, "Rate not found in Frankfurter response");
        }
    }

    Ok(count)
}

/// Background task that fetches exchange rates on startup and then every 24 hours.
pub async fn start_background_fetcher(db: Arc<Database>, quote_currencies: Vec<String>) {
    let currencies: Vec<&str> = quote_currencies.iter().map(|s| s.as_str()).collect();
    match fetch_and_store_rates(&db, &currencies).await {
        Ok(n) => tracing::info!("Initial exchange rate fetch complete: {} rates stored", n),
        Err(e) => tracing::warn!("Initial exchange rate fetch failed: {}", e),
    }

    let mut interval = tokio::time::interval(Duration::from_secs(24 * 3600));
    loop {
        interval.tick().await;
        let currencies: Vec<&str> = quote_currencies.iter().map(|s| s.as_str()).collect();
        match fetch_and_store_rates(&db, &currencies).await {
            Ok(n) => tracing::info!("Daily exchange rate fetch complete: {} rates stored/updated", n),
            Err(e) => tracing::warn!("Daily exchange rate fetch failed: {}", e),
        }
    }
}
