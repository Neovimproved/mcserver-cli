{
  description = "A very basic flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = "mcserver";
        version = "0.2.3";
        cargoLock.lockFile = ./Cargo.lock;
        src = pkgs.lib.cleanSource ./.;

        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

        buildInputs = with pkgs; [
          openssl
        ];
      };

      devShells.${system}.default = pkgs.mkShell {
        # A recent stable rust toolchain will be added
        inputsFrom = [ self.packages.${system}.default ];

        nativeBuildInputs = with pkgs; [
          rust-analyzer
        ];
      };
    };
}
