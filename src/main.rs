use quantum_goldchain::cli::commands::run_cli;
use quantum_goldchain::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration
    let config = Config::load().unwrap_or_default();
    
    // Run CLI (logging will be initialized there based on debug flag)
    run_cli(config).await?;
    
    Ok(())
}
