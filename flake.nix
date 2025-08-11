{
  description = "A BLE Heart Rate Monitor bridge for Social VR, OBS, Data Logging, and more! ";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [rust-overlay.overlays.default];
        };
      in {
        packages = rec {
          iron_heart = pkgs.callPackage ./default.nix {};
          default = iron_heart;
        };

        devShells.default = pkgs.mkShell rec {
          nativeBuildInputs = with pkgs; [
            rust-bin.stable.latest.default
            pkg-config
            dbus
            openssl
            rustup # for jetbrains IDE
            rust-analyzer
          ];

          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath nativeBuildInputs;
        };
      }
    );
}
