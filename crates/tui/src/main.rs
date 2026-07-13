//! Binary entry point for mimofan.

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    mimofan::run().await
}
