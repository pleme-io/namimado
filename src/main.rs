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
    /// Fetch a URL, run the full nami-core Lisp substrate pipeline
    /// (transforms + effects + derived + agents + components), and
    /// print what fired. Headless — no window, no GPU. Useful for
    /// smoke-testing `~/.config/namimado/*.lisp` without the GUI.
    #[cfg(feature = "browser-core")]
    Navigate { url: String },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Mcp) => {
            let cfg = config::NamimadoConfig::load();
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async {
                if let Err(e) = mcp::run(cfg).await {
                    eprintln!("MCP server error: {e}");
                    std::process::exit(1);
                }
            });
            Ok(())
        }
        #[cfg(feature = "browser-core")]
        Some(Commands::Navigate { url }) => run_headless_navigate(&url),
        None => app::run(&cli.url, cli.devtools),
    }
}

#[cfg(feature = "browser-core")]
fn run_headless_navigate(url: &str) -> anyhow::Result<()> {
    let parsed = url::Url::parse(url)
        .or_else(|_| url::Url::parse(&format!("https://{url}")))?;
    let mut pipeline = crate::webview::substrate::SubstratePipeline::load();
    let outcome = pipeline
        .navigate(&parsed)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    println!("────────────────────────────────────────");
    println!(" namimado navigate  {}", outcome.final_url);
    println!(" {} bytes fetched", outcome.fetched_bytes);
    if let Some(t) = &outcome.title {
        println!(" title: {t}");
    }
    println!("────────────────────────────────────────");

    let r = &outcome.report;
    if let Some(route) = &r.routes_matched {
        println!("route matched:  {route}");
    }
    if !r.queries_dispatched.is_empty() {
        println!("queries:        {}", r.queries_dispatched.join(", "));
    }
    if !r.frameworks.is_empty() {
        let fws: Vec<String> = r
            .frameworks
            .iter()
            .map(|(f, c)| format!("{f} ({c:.2})"))
            .collect();
        println!("frameworks:     {}", fws.join(", "));
    }
    println!("effects fired:  {}", r.effects_fired);
    println!("agents fired:   {}", r.agents_fired);
    println!("transforms:     {}", r.transforms_applied);
    for hit in &r.transform_hits {
        println!("  • {hit}");
    }
    if !r.state_snapshot.is_empty() {
        println!("state cells:");
        for (k, v) in &r.state_snapshot {
            println!("  {k} = {v}");
        }
    }
    if !r.derived_snapshot.is_empty() {
        println!("derived values:");
        for (k, v) in &r.derived_snapshot {
            println!("  {k} = {v}");
        }
    }
    Ok(())
}
