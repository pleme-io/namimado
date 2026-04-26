{
  description = "Namimado (波窓) — desktop web browser with Servo engine + garasu chrome";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-25.11";
    crate2nix.url = "github:nix-community/crate2nix";
    flake-utils.url = "github:numtide/flake-utils";
    substrate = {
      url = "github:pleme-io/substrate";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crate2nix,
      flake-utils,
      substrate,
    }:
    (import "${substrate}/lib/rust-tool-release-flake.nix" {
      inherit nixpkgs crate2nix flake-utils;
    })
      {
        toolName = "namimado";
        src = self;
        repo = "pleme-io/namimado";
        module = {
          description = "Namimado (波窓) — desktop web browser with Servo engine + garasu chrome";
          withMcp = true;
          withHttp = true;
          defaultHttpAddr = "127.0.0.1:7860";
        };
      };
}
