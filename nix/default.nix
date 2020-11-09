{ sources ? import ./sources.nix }:
let
  overlay = self: pkgs: {
    inherit (import sources.niv { }) niv;
    pre-commit-hooks-nix = import sources."pre-commit-hooks.nix";
    inherit (import sources.gitignore { inherit (pkgs) lib; }) gitignoreSource;
    pre-commit-check = self.pre-commit-hooks-nix.run {
      hooks.nixfmt.enable = true;
      hooks.rustfmt.enable = true;
      hooks.clippy.enable =
        false; # TODO Disable temporarily due to version mismatch
      src = self.gitignoreSource ../.;
    };
  };
in import sources.nixpkgs {
  overlays = [ overlay ];
  config = { };
}
