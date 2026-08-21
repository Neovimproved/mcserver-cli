{
  description = "A very basic flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    {
      packages = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" "hello-linux" ] (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "mcserver";
            version = "0.2.3";
            cargoLock.lockFile = ./Cargo.lock;
            src = pkgs.lib.cleanSource ./.;

            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            buildInputs = with pkgs; [
              openssl
              mcrcon
            ];
          };
        }
      );

      devShells = nixpkgs.lib.genAttrs [ "x86-64_linux" "aarch64-linux" "hello-linux" ] (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = {
            # A recent stable rust toolchain will be added
            inputsFrom = [ self.packages.${system}.default ];

            nativeBuildInputs = with pkgs; [
              rust-analyzer
            ];
          };
        }
      );
    };
}
