{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    rust-overlay.url = "github:oxalica/rust-overlay";
    import-cargo.url = "github:edolstra/import-cargo";
  };

  outputs = { self, nixpkgs, flake-utils, naersk, rust-overlay, import-cargo }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = (import nixpkgs) {
          inherit system;
        };

        naersk' = pkgs.callPackage naersk { };

        rustPkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        wasmTarget = "wasm32-unknown-unknown";
        rustWithWasmTarget = rustPkgs.rust-bin.stable.latest.default.override {
          targets = [ wasmTarget ];
        };

      in
      rec {
        # For `nix build` & `nix run`
        defaultPackage = packages.all;

        packages = {
          all = pkgs.writeShellScriptBin "roxide" ''
            ROCKET_FRONT_SOURCES=${packages.frontend} exec ${packages.backend}/bin/roxide-backend
          '';

          backend = naersk'.buildPackage {
            root = ./.;
            cargoBuildOptions = x: x ++ [ "-p" "roxide-backend" ];
            cargoTestOptions = x: x ++ [ "-p" "roxide-backend" ];
            nativeBuildInputs = with pkgs; [ openssl pkgconfig ];
          };

          frontend = pkgs.stdenv.mkDerivation {
            pname = "roxide-frontend";
            version = "0.1.0";

            src = ./roxide-frontend;

            nativeBuildInputs = with pkgs; [
              rustWithWasmTarget
              trunk
              cargo
              wasm-bindgen-cli
              sass
              (import-cargo.builders.importCargo {
                lockFile = ./Cargo.lock;
                inherit pkgs;
              }).cargoHome
            ];

            buildPhase = ''
              export TRUNK_TOOLS_wasm_bindgen=$(wasm-bindgen --version | cut -f2 -d' ')
              trunk build --release
            '';

            installPhase = ''
              mkdir -p $out/
              mv dist/* $out
            '';
          };
        };

        # For `nix develop`
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [ rustc cargo openssl pkgconfig ];
        };
      }
    );
}
