use clap::Parser;

#[tokio::main]
async fn main() -> harpe_cli::CliResult<()> {
    harpe_cli::run(harpe_cli::Cli::parse()).await
}
