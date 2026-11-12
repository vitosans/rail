{
  description = "Rail — modern email, native to your terminal";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      # The toolchain comes from rust-toolchain.toml, so Nix uses the same compiler and the
      # same components (clippy, rustfmt) as CI and the other development paths.
      toolchainFor = pkgs: pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      workspaceVersion = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;

      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          let
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            };
          in
          f pkgs
        );
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          rustToolchain = toolchainFor pkgs;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
        in
        {
          default = rustPlatform.buildRustPackage {
            pname = "rail";
            version = workspaceVersion;
            src = self;

            cargoLock.lockFile = ./Cargo.lock;

            # The TLS stack is rustls, so no OpenSSL is needed. rusqlite is built with its
            # bundled SQLite, but it also keeps the `-lsqlite3` link directive (its default
            # `ffi-sqlite-wasm-rs` feature), so the library nixpkgs provides is what the final
            # link resolves against.
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.sqlite ];

            # The test suite binds loopback sockets and drives the binary in a pty, and the
            # Nix sandbox has no network. Run `cargo test --workspace` in `nix develop`.
            doCheck = false;

            meta = {
              description = "A terminal mail client for Gmail, Microsoft 365 and IMAP/SMTP";
              homepage = "https://github.com/vitosans/rail";
              license = with pkgs.lib.licenses; [
                mit
                asl20
              ];
              mainProgram = "rail";
              # Rail is tested on Linux and macOS only.
              platforms = [
                "x86_64-linux"
                "aarch64-linux"
                "x86_64-darwin"
                "aarch64-darwin"
              ];
            };
          };
        }
      );

      devShells = forAllSystems (
        pkgs:
        let
          rustToolchain = toolchainFor pkgs;
        in
        {
          default = pkgs.mkShell {
            name = "rail";

            packages = [
              rustToolchain
              pkgs.cargo-deny
              pkgs.cargo-llvm-cov
              # The crate the binary links `-lsqlite3` against, as in the package.
              pkgs.sqlite
              pkgs.pkg-config
              pkgs.python3
            ];

            # macOS needs a higher descriptor limit for the test suite to spawn daemons.
            shellHook = ''
              ulimit -n 10240 2>/dev/null || true
            '';
          };
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt);

      checks = forAllSystems (pkgs: {
        package = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      });
    };
}
