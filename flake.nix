{
  description = "ai-commit, built static and published as a release binary";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs, ... }:
    let
      # Only x86_64-linux assets are published (GitHub runners are x86_64).
      systems = [ "x86_64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        rec {
          default = pkgs.callPackage ./package.nix { };
          static = pkgs.callPackage ./static.nix { };
          ai-commit = default;
        }
      );

      overlays.default = final: _prev: {
        ai-commit = final.callPackage ./package.nix { };
      };
    };
}
