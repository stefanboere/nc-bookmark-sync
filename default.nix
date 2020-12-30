let
  pkgs = import ./nix { };

  nc-bookmark-sync = pkgs.rustPlatform.buildRustPackage {
    pname = "nc-bookmark-sync";
    version = "0.0.1";

    buildInputs = with pkgs; [ openssl ];
    nativeBuildInputs = with pkgs; [ pkgconfig ];

    src = pkgs.gitignoreSource ./.;

    cargoSha256 = "0c3g79vqqmb400jci6qi2ymsmlz23knxq795dp4c9hqggs0hp185";

    meta = with pkgs.stdenv.lib; {
      description = "Sync Nextcloud bookmarks with a plain text file.";
      license = licenses.mit;
    };
  };

  # Development shell
  devshell = pkgs.stdenv.mkDerivation {
    name = "rust-env";
    nativeBuildInputs = with pkgs; [
      rustc
      cargo
      niv
      rls
      rustfmt
      openssl
      pkgconfig
    ];
    inherit (pkgs.pre-commit-check) shellHook;
  };

in { inherit devshell nc-bookmark-sync; }
