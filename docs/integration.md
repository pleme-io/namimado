# Integration — wire namimado into your nix rebuild

Drop these snippets into your private `nix` repo (or `blackmatter-pleme`)
so `nix run .#darwin-rebuild` / `nix run .#home-manager` installs
namimado and registers its MCP server with every configured AI agent.

## 1. Pin inputs in your top-level flake

```nix
# flake.nix
{
  inputs = {
    # ...existing inputs...
    namimado = {
      url = "github:pleme-io/namimado";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nami = {
      url = "github:pleme-io/nami";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
}
```

## 2. Add the HM modules to your user config

```nix
# home.nix (or equivalent)
{ inputs, pkgs, ... }: {
  imports = [
    inputs.namimado.homeManagerModules.default
    inputs.nami.homeManagerModules.default
  ];

  nixpkgs.overlays = [
    inputs.namimado.overlays.default
    inputs.nami.overlays.default
  ];

  programs.namimado = {
    enable = true;
    enableMcpBin = true;          # installs ~/.local/bin/namimado-mcp
    enableHttpService = true;     # launchd/systemd /ui + /typescape
    httpAddr = "127.0.0.1:7860";
  };

  programs.nami = {
    enable = true;
    enableMcpBin = true;
  };
}
```

## 3. Register the MCP servers with blackmatter-anvil

Once `enableMcpBin` lays down `~/.local/bin/namimado-mcp` and
`~/.local/bin/nami-mcp`, point anvil at them:

```nix
# anywhere the anvil module is configured
{ config, ... }: {
  blackmatter.components.anvil = {
    enable = true;

    mcp.servers.namimado = {
      command = "${config.home.homeDirectory}/.local/bin/namimado-mcp";
      args = [];
      enableFor = [ "claude-code" "cursor" "rovodev" ];
      description = "Lisp-substrate browser — typescape + dom + navigate tools";
    };

    mcp.servers.nami = {
      command = "${config.home.homeDirectory}/.local/bin/nami-mcp";
      args = [];
      enableFor = [ "claude-code" ];
      description = "TUI browser — same substrate surface, no GPU window";
    };
  };
}
```

Anvil writes the correct config file for each registered agent
(`~/.cursor/mcp.json`, `~/.rovodev/mcp.json`, Claude Code
settings, …) — one declaration, N agents.

## 4. Rebuild

```sh
cd ~/code/github/drzln/nix
nix run .#darwin-rebuild        # or .#home-manager
```

After the rebuild:

- `namimado https://example.com` — GPU window
- `namimado serve` is already running on :7860 (from
  `enableHttpService`); open http://127.0.0.1:7860/ui
- Claude Code picks up the `namimado` and `nami` MCP servers
  automatically — ask it to call `navigate`, `get_typescape`,
  `get_dom_sexp`, `reload`, etc.
- `nami` — TUI browser with the same substrate pipeline

## Verification

```sh
# Rebuild succeeded?
namimado --version
nami --version

# HTTP face live?
curl -s http://127.0.0.1:7860/typescape | jq '.hash, .normalize_packs | length'

# MCP bin on path?
ls -l ~/.local/bin/namimado-mcp ~/.local/bin/nami-mcp

# Anvil registered it?
cat ~/.cursor/mcp.json | jq '.mcpServers.namimado'
cat ~/.claude.json | jq '.mcpServers.namimado'
```

## Optional — user rule packs + WASM agents

The HM module doesn't prescribe substrate content. Drop your own
into `~/.config/namimado/` any time:

```
~/.config/namimado/
  extensions.lisp          ;; (defstate) (defeffect) (defnormalize) ...
  transforms.lisp          ;; (defdom-transform)
  aliases.lisp             ;; (defframework-alias)
  substrate.d/
    html5.lisp             ;; starter packs from examples/
    shadcn.lisp
    mui.lisp
    bootstrap.lisp
    tailwind.lisp
    shadcn-emit.lisp
    my-custom.lisp
  wasm/
    recipe-extractor.wasm  ;; referenced by (defwasm-agent :wasm "…")
```

Call `POST /reload` or the MCP `reload` tool to pick up edits
without restart.
