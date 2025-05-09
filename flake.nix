{
  description = "Sync Nextcloud bookmarks with the local file system";

  inputs = {
    nixpkgs.url = "nixpkgs/nixpkgs-unstable";

    naersk.url = "github:nix-community/naersk";

    pre-commit-hooks.url = "github:cachix/pre-commit-hooks.nix";

    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, naersk, pre-commit-hooks, flake-utils }:
    {
      overlay = final: prev: {
        inherit (self.packages.${final.system}) nc-bookmark-sync;
      };
      hmModule = import ./modules/nc-bookmark-sync.nix;
    } // flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        naersk-lib = naersk.lib.${system};
      in rec {
        checks = {
          pre-commit-check = pre-commit-hooks.lib.${system}.run {
            src = ./.;
            hooks = {
              nixfmt-classic.enable = true;
              rustfmt.enable = true;
            };
          };
        };

        # `nix build`
        packages.nc-bookmark-sync = naersk-lib.buildPackage {
          pname = "nc-bookmark-sync";
          version = "0.0.1";

          buildInputs = with pkgs; [ openssl ];
          nativeBuildInputs = with pkgs; [ pkg-config ];

          root = ./.;

          meta = with pkgs.lib; {
            description = "Sync Nextcloud bookmarks with a plain text file.";
            license = licenses.mit;
          };
        };
        defaultPackage = packages.nc-bookmark-sync;

        # `nix run`
        apps.nc-bookmark-sync =
          flake-utils.lib.mkApp { drv = packages.nc-bookmark-sync; };
        defaultApp = apps.nc-bookmark-sync;

        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustc
            cargo
            rust-analyzer
            rustfmt
            openssl
            pkg-config
          ];
          inherit (self.checks.${system}.pre-commit-check) shellHook;
        };
      });
}
