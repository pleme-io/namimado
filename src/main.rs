mod api;
mod app;
mod browser;
mod chrome;
mod config;
#[cfg(feature = "gpu-chrome")]
mod gpu;
#[cfg(feature = "http-server")]
mod http_server;
mod input;
mod ipc;
mod mcp;
mod render;
mod scripting;
mod service;
mod typescape;
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
    /// Start the HTTP REST API server. Spec: ./openapi.yaml. All
    /// endpoints delegate into the same `NamimadoService` that MCP
    /// tools use — one spec, many faces.
    #[cfg(feature = "http-server")]
    Serve {
        /// Bind address. Defaults to 127.0.0.1:7860.
        #[arg(long, default_value = "127.0.0.1:7860")]
        addr: String,
    },
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
        #[cfg(feature = "http-server")]
        Some(Commands::Serve { addr }) => {
            let addr: std::net::SocketAddr = addr.parse()?;
            // Construct service BEFORE the tokio runtime — nami-core's
            // blocking reqwest client can't be initialised inside an
            // async context (it spins up its own nested runtime).
            let service = crate::service::NamimadoService::new();
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(crate::http_server::serve(service, addr))
        }
        #[cfg(feature = "browser-core")]
        Some(Commands::Navigate { url }) => run_headless_navigate(&url),
        None => run_default(&cli.url, cli.devtools),
    }
}

#[cfg(feature = "gpu-chrome")]
fn run_default(initial_url: &str, _devtools: bool) -> anyhow::Result<()> {
    gpu::run(initial_url)
}

#[cfg(not(feature = "gpu-chrome"))]
fn run_default(initial_url: &str, devtools: bool) -> anyhow::Result<()> {
    app::run(initial_url, devtools)
}

#[cfg(feature = "browser-core")]
fn run_headless_navigate(url: &str) -> anyhow::Result<()> {
    use crate::api::NavigateRequest;

    let service = crate::service::NamimadoService::new();
    let resp = service.navigate(NavigateRequest {
        url: url.to_owned(),
    })?;

    println!("────────────────────────────────────────");
    println!(" namimado navigate  {}", resp.final_url);
    println!(" {} bytes fetched", resp.fetched_bytes);
    if let Some(t) = &resp.title {
        println!(" title: {t}");
    }
    println!("────────────────────────────────────────");

    let r = &resp.report;
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
            .map(|f| format!("{} ({:.2})", f.name, f.confidence))
            .collect();
        println!("frameworks:     {}", fws.join(", "));
    }
    println!("effects fired:  {}", r.effects_fired);
    println!("agents fired:   {}", r.agents_fired);
    println!("transforms:     {}", r.transforms_applied);
    println!(
        "inline-lisp:    {} ok · {} err",
        r.inline_lisp_evaluated, r.inline_lisp_failed
    );
    println!("normalize:      {} applied", r.normalize_applied);
    for hit in &r.normalize_hits {
        println!("  · {hit}");
    }
    println!("wasm-agents:    {} fired", r.wasm_agents_fired);
    for hit in &r.wasm_agent_hits {
        println!("  · {hit}");
    }
    for hit in &r.transform_hits {
        println!("  • {hit}");
    }
    if !r.state_snapshot.is_empty() {
        println!("state cells:");
        for cell in &r.state_snapshot {
            println!("  {} = {}", cell.name, cell.value);
        }
    }
    if !r.derived_snapshot.is_empty() {
        println!("derived values:");
        for cell in &r.derived_snapshot {
            println!("  {} = {}", cell.name, cell.value);
        }
    }
    Ok(())
}
