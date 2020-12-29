let
  pkgs = import ./nix { };

  nc-bookmark-sync = pkgs.rustPlatform.buildRustPackage {
    pname = "nc-bookmark-sync";
    version = "0.0.1";

    buildInputs = with pkgs; [ openssl ];
    nativeBuildInputs = with pkgs; [ pkgconfig ];

    src = pkgs.gitignoreSource ./.;

    cargoSha256 = "1x102fwbh2jy0ywn9ab9mr1b5j1ly91k2j7avz1ipx15chznxb5c";

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
