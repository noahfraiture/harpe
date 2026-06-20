use clap::Parser;

#[tokio::main]
async fn main() -> harpe_cli::CliResult<()> {
    let args = harpe_cli::tui::TuiArgs::parse();
    harpe_cli::tui::run(args).await
}
