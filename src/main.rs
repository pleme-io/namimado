mod app;
mod browser;
mod chrome;
mod config;
mod ipc;
mod webview;

use clap::Parser;

#[derive(Parser)]
#[command(name = "namimado", about = "Desktop web browser — Servo/wry + garasu chrome")]
struct Cli {
    /// URL to open on launch
    #[arg(default_value = "about:blank")]
    url: String,

    /// Enable developer tools
    #[arg(long)]
    devtools: bool,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    app::run(&cli.url, cli.devtools)
}
