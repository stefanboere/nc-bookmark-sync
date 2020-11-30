let
  pkgs = import ./nix { };

  packages = { };

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

in {
  inherit devshell;

} // packages
