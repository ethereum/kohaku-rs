{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        rustToolchain = pkgs.rust-bin.stable."1.98.0".default.override {
          extensions = [
            "rust-src"
            "llvm-tools"
          ];
          targets = [ "wasm32-unknown-unknown" ];
        };

        rustfmtNightly = pkgs.rust-bin.nightly.latest.rustfmt;

        alto = pkgs.callPackage ./nix/alto/package.nix { };
      in
      {
        devShells = {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rustfmtNightly
              rustToolchain
              rust-analyzer
              bacon
              cargo-audit
              cargo-autoinherit
              cargo-sort
              cargo-insta

              foundry
              alto

              just
            ];
          };

          ci = pkgs.mkShell {
            packages = with pkgs; [
              rustfmtNightly
              rustToolchain
              cargo-audit

              foundry
              alto
            ];
          };
        };
      }
    );
}
