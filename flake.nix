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
      # nixpkgs 26.11 dropped Intel macOS (x86_64-darwin); Intel Macs use the release binary or
      # `cargo install` instead.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
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

            # The TLS stack is rustls, so no OpenSSL is needed. nixpkgs' cargo setup hook sets
            # LIBSQLITE3_SYS_USE_PKG_CONFIG=1, so the package links nixpkgs' SQLite (which has
            # FTS5) instead of the copy rusqlite bundles, as nixpkgs does for every Rust package.
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ pkgs.sqlite ];

            # The daemon tells two builds of one version apart by the build stamp, which
            # rail-core's build.rs takes from SOURCE_DATE_EPOCH. The stdenv fixes that at 1980,
            # so every Nix build would share one stamp and a newer `rail` would keep talking to
            # an older daemon. The commit time keeps builds reproducible and still ordered.
            env.SOURCE_DATE_EPOCH = toString self.lastModified;

            # The test suite binds loopback sockets and drives the binary in a pty, and the
            # Nix sandbox has no network. Run `cargo test --workspace --locked` in `nix develop`.
            doCheck = false;

            meta = {
              description = "A terminal mail client for Gmail and Microsoft 365";
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
              # The same SQLite as the package, found through pkg-config.
              pkgs.sqlite
              pkgs.pkg-config
              pkgs.python3
            ];

            # The test suite spawns many daemons; macOS's default descriptor limit is too low for it.
            # Raising it is harmless elsewhere.
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
