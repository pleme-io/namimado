mod app;
mod browser;
mod chrome;
mod config;
mod input;
mod ipc;
mod mcp;
mod render;
mod scripting;
mod webview;

use clap::Parser;

#[derive(Parser)]
#[command(name = "namimado", about = "Desktop web browser — Servo/wry + garasu chrome")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// URL to open on launch
    #[arg(default_value = "about:blank")]
    url: String,

    /// Enable developer tools
    #[arg(long)]
    devtools: bool,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Start the MCP server (stdio transport).
    Mcp,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    if let Some(Commands::Mcp) = cli.command {
        let cfg = config::NamimadoConfig::load();
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            if let Err(e) = mcp::run(cfg).await {
                eprintln!("MCP server error: {e}");
                std::process::exit(1);
            }
        });
        return Ok(());
    }

    app::run(&cli.url, cli.devtools)
}
