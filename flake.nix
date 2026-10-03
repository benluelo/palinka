{
  description = "Rust flake template";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    zig-overlay = {
      url = "github:mitchellh/zig-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
    };
    treefmt-nix.url = "github:numtide/treefmt-nix";
  };
  outputs =
    inputs@{
      nixpkgs,
      rust-overlay,
      zig-overlay,
      flake-parts,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      imports = [ inputs.treefmt-nix.flakeModule ];

      perSystem =
        {
          config,
          self',
          inputs',
          pkgs,
          system,
          ...
        }:
        let
          dbg =
            value:
            builtins.trace (
              if value ? type && value.type == "derivation" then
                "derivation: ${value}"
              else
                pkgs.lib.generators.toPretty { } value
            ) value;

          crane = {
            lib = ((inputs.crane.mkLib pkgs).overrideToolchain (_: self'.packages.rust-nightly)).overrideScope (
              final: prev: {
                stdenvSelector = p: p.clangStdenv;
              }
            );
          };

          nist-vectors = pkgs.fetchzip {
            url = "https://csrc.nist.gov/CSRC/media/Projects/Cryptographic-Algorithm-Validation-Program/documents/sha3/sha-3bytetestvectors.zip";
            stripRoot = false;
            hash = "sha256-nWNYO4H2piqf6CW7NJfqc4+DHzByYoNbbjGE3QeO4uc=";
          };
          blake3-vectors = builtins.fetchurl {
            name = "test_vectors.json";
            url = "https://raw.githubusercontent.com/BLAKE3-team/BLAKE3/refs/heads/master/test_vectors/test_vectors.json";
            sha256 = "sha256:097n6bdn9l67jnjqsr6gg2pg7acr3bf7rbrjwvbfcxycmjl1xffw";
          };
          src =
            extra:
            let
              unfilteredRoot = ./.;
            in
            pkgs.lib.fileset.toSource {
              root = unfilteredRoot;
              fileset = pkgs.lib.fileset.unions [
                (crane.lib.fileset.commonCargoSources unfilteredRoot)
                ./c
                ./zig
                extra
              ];
            };
          crateAttrs = {
            nativeBuildInputs = [
              pkgs.pkg-config
              pkgs.rustPlatform.bindgenHook
            ];
            buildInputs = [
              pkgs.llvmPackages_latest.libclang.lib
              pkgs.llvmPackages_latest.libllvm
              pkgs.llvmPackages_latest.lld
              pkgs.llvmPackages_latest.bintools
              pkgs.clangStdenv.cc.libc
              pkgs.zigpkgs.master
            ];
            LIBCLANG_PATH = "${pkgs.llvmPackages_latest.libclang.lib}/lib";
            preBuild = ''
              # zig needs a $HOME dir for caching (non-configurable)
              export ZIG_GLOBAL_CACHE_DIR=.
            '';
          };
          build = crane.lib.buildPackage (
            crateAttrs
            // {
              src = src pkgs.lib.fileset.empty;
              doCheck = false;
              cargoBuildCommand = "cargo build --profile lto -Ftracing-off";
              meta.mainProgram = "palinka";
            }
          );
          buildObject =
            cefreFile:
            pkgs.clangStdenv.mkDerivation {
              name = "${baseNameOf cefreFile}.o";
              src = cefreFile;
              dontUnpack = true;
              buildInputs = [ build ];
              buildPhase = ''
                palinka build ${cefreFile} -o a.out
              '';
              installPhase = ''
                mv ./a.out "$out"
              '';
            };

          cefreTreeSitter = pkgs.stdenv.mkDerivation {
            name = "cefre.so";
            src = ./tree-sitter-cefre;
            buildInputs = [ pkgs.tree-sitter ];
            buildPhase = ''
              export HOME=.
              tree-sitter build -o "$out"
              chmod +x "$out"
            '';
          };
        in
        {
          _module.args.pkgs = import nixpkgs {
            inherit system;
            overlays = [
              rust-overlay.overlays.default
              zig-overlay.overlays.default
            ];
          };

          packages = {
            rust-nightly = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
            default = build;
            inherit build;
          };
          apps =
            builtins.mapAttrs
              (name: value: {
                type = "app";
                program = value;
                meta = value.meta or { };
              })
              {
                mini-benchmark =
                  let
                    cmd = ''${pkgs.lib.getExe self'.packages.build} run --obj ${buildObject ./tests/sha3-256.cfr} --input-file "''${1:-${./random.bin}}"'';
                  in
                  pkgs.writeShellApplication {
                    name = "run";
                    text = ''
                      echo "Palinka VM Implementation Benchmark"
                      echo ""
                      echo "Each implementation will run the same program: sha3-256 hash of 1mb of random data."
                      echo "Alternatively, a different input file can be provided as the first argument to this script."
                      echo ""

                      echo "Running rust..."
                      time ${cmd} -i rust
                      echo

                      echo "Running rust-tc..."
                      time ${cmd} -i rust-tc
                      echo

                      echo "Running zig..."
                      time ${cmd} -i zig
                      echo

                      echo "Running c..."
                      time ${cmd} -i c
                      echo
                    '';
                    meta.description = "Run a mini comparison benchmark of all implementations.";
                  };
                install-helix-grammars = pkgs.writeShellApplication {
                  name = "install-helix-grammars";
                  text = ''
                    rm -f "$HOME/.config/helix/runtime/grammars/cefre.so"
                    cp --no-preserve=mode ${cefreTreeSitter} "$HOME/.config/helix/runtime/grammars/cefre.so"
                    mkdir -p "$HOME/.config/helix/runtime/queries/cefre"
                    cp --no-preserve=mode -TR ${./tree-sitter-cefre/queries} "$HOME/.config/helix/runtime/queries/cefre"
                  '';
                  meta.description = "Run a mini comparison benchmark of all implementations.";
                };
                generate-tree-sitter-grammars = pkgs.writeShellApplication {
                  name = "generate-tree-sitter-grammars";
                  runtimeInputs = [ pkgs.tree-sitter ];
                  text = ''
                    pushd tree-sitter-cefre/ >/dev/null 
                    tree-sitter generate --abi 14
                    popd >/dev/null
                  '';
                  meta.description = "Run a mini comparison benchmark of all implementations.";
                };
                install-nist-vectors = pkgs.writeShellApplication {
                  name = "install-nist-vectors";
                  text = ''
                    rm -r .nist-vectors/ 2>/dev/null || echo ""
                    mkdir -p .nist-vectors
                    cp -r --no-preserve=mode ${nist-vectors}/* .nist-vectors
                  '';
                  meta.description = "Install the NIST sha3 test vectors in the current directory in .nist-vectors/.";
                };
                install-blake3-vectors = pkgs.writeShellApplication {
                  name = "install-blake3-vectors";
                  text = ''
                    rm -r .blake3-vectors/ 2>/dev/null || echo ""
                    mkdir -p .blake3-vectors
                    cp -r --no-preserve=mode ${blake3-vectors} .blake3-vectors/test_vectors.json
                  '';
                  meta.description = "Install the blake3 test vectors in the current directory in .blake3-vectors/.";
                };
              };
          checks = {
            default =
              let
                attrs = crateAttrs // {
                  src = src ./tests;
                };
              in
              crane.lib.cargoTest (
                attrs
                // {
                  preBuild = ''
                    ${attrs.preBuild}

                    ${self'.apps.install-blake3-vectors.program}
                    ${self'.apps.install-nist-vectors.program}
                  '';

                  cargoTestExtraArgs = "-- --nocapture";

                  cargoArtifacts = crane.lib.buildDepsOnly attrs;
                }
              );
          };
          devShells = {
            default = pkgs.mkShellNoCC.override { stdenv = pkgs.clangStdenv; } {
              buildInputs = [
                pkgs.llvmPackages_latest.libclang.lib
                pkgs.llvmPackages_latest.libllvm
                pkgs.llvmPackages_latest.lld
                pkgs.llvmPackages_latest.bintools
                pkgs.clangStdenv.cc.libc
                self'.packages.rust-nightly
              ]
              ++ [ pkgs.zigpkgs.master ]
              ++ (with pkgs; [
                cargo-fuzz
                jq
                moreutils
                nixd
                nil
                tree-sitter
                nodejs
                typescript-language-server
                graphviz
                samply
                cargo-pgo
                cargo-criterion
                hexyl
                zig
                zls
                clang-tools
              ]);
              LIBCLANG_PATH = "${pkgs.llvmPackages_latest.libclang.lib}/lib";
              CUSTOM_LIBFUZZER_PATH =
                if pkgs.stdenv.hostPlatform.isx86_64 then
                  "${pkgs.llvmPackages_latest.compiler-rt}/lib/linux/libclang_rt.fuzzer-x86_64.a"
                else
                  "${pkgs.llvmPackages_latest.compiler-rt}/lib/linux/libclang_rt.fuzzer-aarch64.a";
              nativeBuildInputs = [
                pkgs.pkg-config
                pkgs.rustPlatform.bindgenHook
                config.treefmt.build.wrapper
              ]
              ++ pkgs.lib.attrsets.attrValues config.treefmt.build.programs;
            };
          };

          treefmt = {
            projectRootFile = "flake.nix";
            programs = {
              rustfmt.enable = true;
              nixfmt.enable = true;
            };
            settings = {
              rustfmt = { };
            };
          };
        };
    };
}
