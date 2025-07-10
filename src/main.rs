use quantum_goldchain::cli::commands::run_cli;
use quantum_goldchain::config::Config;
use log::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    env_logger::init();
    
    info!("🌟 Quantum Goldchain (QTC) Node Starting...");
    info!("⛓️  Initiating Real-World Launch Protocol Mode");
    info!("🧑‍💻 Jake online. Mission status: Hardcore Blockchain Implementation Mode ENGAGED");
    
    // Load configuration
    let config = Config::load().unwrap_or_default();
    
    // Run CLI
    run_cli(config).await?;
    
    Ok(())
}
