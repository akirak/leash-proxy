{
  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    nixpkgs.url = "nixpkgs-unstable";
    # systems.url = "github:nix-systems/default";

    treefmt-nix.url = "github:numtide/treefmt-nix";

    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  # Add settings for your binary cache.
  # nixConfig = {
  #   extra-substituters = [
  #   ];
  #   extra-trusted-public-keys = [
  #   ];
  # };

  outputs =
    inputs@{ nixpkgs, flake-parts, ... }:
    let
      # For details on these options, See
      # https://github.com/oxalica/rust-overlay?tab=readme-ov-file#cheat-sheet-common-usage-of-rust-bin
      #
      # Channel of the Rust toolchain (stable or beta).
      rustChannel = "stable";
      # Version (latest or specific date/semantic version)
      rustVersion = "latest";
      # Profile (default or minimal)
      rustProfile = "default";

      # Set the Erlang version
      beamVersion = "beam28Packages";
      # Set the Elixir version
      elixirVersion = "elixir_1_20";
    in
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = nixpkgs.lib.systems.flakeExposed;

      imports = [
        inputs.treefmt-nix.flakeModule
      ];

      perSystem =
        {
          config,
          system,
          pkgs,
          lib,
          craneLib,
          commonArgs,
          ...
        }:
        {
          _module.args = {
            pkgs = import nixpkgs {
              inherit system;
              overlays = [
                inputs.rust-overlay.overlays.default
                (
                  final: _:
                  let
                    beamPackages = final.${beamVersion}.overrideScope (
                      _: prev: {
                        elixir = prev.${elixirVersion};
                      }
                    );
                  in
                  {
                    inherit beamPackages;
                    inherit (beamPackages) erlang elixir hex;
                  }
                )
              ];
            };
            craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (
              pkgs: pkgs.rust-bin.${rustChannel}.${rustVersion}.${rustProfile}
            );
            commonArgs = {
              pname = "leash_proxy";
              version = "0.1";

              # Depending on your code base, you may have to customize the
              # source filtering to include non-standard files during the build.
              # See
              # https://crane.dev/source-filtering.html?highlight=source#source-filtering
              src = craneLib.cleanCargoSource (craneLib.path ./rust);

              nativeBuildInputs = with pkgs; [ pkg-config ];
              buildInputs = with pkgs; [ openssl ];
            };
          };

          packages = {
            # Build the executable package.
            leash-proxy = craneLib.buildPackage (
              commonArgs
              // {
                cargoArtifacts = craneLib.buildDepsOnly commonArgs;
                # Ship the default Cedar schema/policy set alongside the binary;
                # the proxy resolves `policies/` relative paths at runtime.
                postInstall = ''
                  mkdir -p $out/share/leash-proxy
                  cp -r ${./rust/policies} $out/share/leash-proxy/policies
                  chmod -R u+w $out/share/leash-proxy
                '';
              }
            );

            leash-collector = pkgs.beamPackages.mixRelease (finalAttrs: {
              pname = "leash-collector";
              version = "0.1.0";

              # Only ship application sources, not local deps, builds or databases.
              src = lib.fileset.toSource {
                root = ./elixir;
                fileset = lib.fileset.unions [
                  ./elixir/assets
                  ./elixir/config
                  ./elixir/lib
                  ./elixir/priv
                  ./elixir/mix.exs
                  ./elixir/mix.lock
                ];
              };

              mixFodDeps = pkgs.beamPackages.fetchMixDeps {
                inherit (finalAttrs) pname version src;
                hash = "sha256-FEAAyF+Ez8GE2bj6YERqp77/Wo5VHd4RCshfwcjy0zE=";
              };

              # Build the SQLite NIF locally instead of downloading prebuilt code.
              buildInputs = [ pkgs.sqlite ];
              env.EXQLITE_USE_SYSTEM = "1";

              # Use Nix-provided asset tools; Mix must not download them in the sandbox.
              preBuild = ''
                echo 'config :esbuild, path: "${lib.getExe pkgs.esbuild}", version: "${pkgs.esbuild.version}"' >> config/config.exs
                echo 'config :tailwind, path: "${lib.getExe pkgs.tailwindcss_4}", version: "${pkgs.tailwindcss_4.version}"' >> config/config.exs
              '';
              postBuild = ''
                mix do deps.loadpaths --no-deps-check + assets.deploy
              '';

              meta = {
                description = "Phoenix audit console and collector for leash-proxy";
                license = lib.licenses.mit;
                mainProgram = "leash_collector";
              };
            });
          };

          devShells.default = craneLib.devShell {
            inputsFrom = [
              # Disable this when you first generate Cargo.lock
              config.packages.leash-proxy
            ];

            packages = [
              pkgs.rust-analyzer-unwrapped
              pkgs.lean4
              pkgs.erlang
              pkgs.elixir
              pkgs.nodejs
              pkgs.corepack
              pkgs.process-compose
              pkgs.beamPackages.expert
            ]
            ++ lib.optional pkgs.stdenv.hostPlatform.isLinux pkgs.inotify-tools
            ++ (lib.optionals pkgs.stdenv.hostPlatform.isDarwin (
              with pkgs.darwin.apple_sdk.frameworks;
              [
                CoreFoundation
                CoreServices
              ]
            ));

            RUST_SRC_PATH = "${
              pkgs.rust-bin.${rustChannel}.${rustVersion}.rust-src
            }/lib/rustlib/src/rust/library";
          };

          treefmt = {
            projectRootFile = "flake.nix";
            settings.excludes = [
              ".direnv/*"
              "rust/target/*"
              # "node_modules/*"
            ];
            programs = {
              actionlint.enable = true;
              nixfmt.enable = true;
              rustfmt.enable = true;
              zizmor.enable = true;
            };
          };
        };
    };
}
