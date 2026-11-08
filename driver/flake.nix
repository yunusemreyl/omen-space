{
  description = "Install the hp-wmi-fan-and-backlight-control kernel modules on NixOS";

  inputs = {
    nixpkgs.url = "nixpkgs/nixos-unstable";
  };
  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      lib = nixpkgs.lib;
      pkgbuildLines = lib.strings.splitString "\n" (builtins.readFile ./PKGBUILD);
      versionDefinition = lib.lists.findFirst (
        line: lib.strings.hasPrefix "pkgver=" line
      ) "unknown" pkgbuildLines;
      version = lib.strings.removePrefix "pkgver=" versionDefinition;
      module =
        {
          stdenv,
          lib,
          kernel,
          nix-gitignore,
        }:
        stdenv.mkDerivation (finalAttrs: {
          pname = "hp-wmi-fan-and-backlight-control";
          inherit version;
          src = lib.cleanSource (
            nix-gitignore.gitignoreSourcePure [
              ./.gitignore
              "result*"
            ] ./.
          );

          postPatch = ''
            sed -i 's@depmod -a@@g' Makefile
          '';

          nativeBuildInputs = kernel.moduleBuildDependencies;

          makeFlags = (kernel.makeFlags or []) ++ [
            "KDIR=${kernel.dev}/lib/modules/${kernel.modDirVersion}/build"
            "M=$(PWD)"
            "INSTALL_MOD_PATH=$(out)"
          ];

          enableParallelBuilding = true;

          installTargets = [ "modules_install" ];

          meta = {
            description = "Linux kernel module for HP Laptops";
            homepage = "https://github.com/Vilez0/hp-wmi-fan-and-backlight-control";
            license = lib.licenses.gpl2;
            maintainers = with lib.maintainers; [ ern775 ];
            platforms = lib.platforms.linux;
          };
        });
    in
    {
      packages.${system} = {
        default = self.packages.${system}.hp-wmi-control;
        hp-wmi-control = module;
      };

      nixosModules.default = (
        { config, lib, ... }:
        let
          inherit (lib) mkEnableOption mkIf;
          cfg = config.hardware.hp-wmi-control;
          callPackage = config.boot.kernelPackages.callPackage;
        in
        {
          options = {
            hardware.hp-wmi-control = {
              enable = mkEnableOption "Enable the hp-wmi-fan-and-backlight-control kernel module";
              victus-15-support.enable = mkEnableOption "Enable forced manual fan control support for Victus 15 laptops";
            };
          };
          config = mkIf cfg.enable {
            boot.extraModulePackages = [
              (callPackage self.packages.${system}.hp-wmi-control { })
            ];
            boot.extraModprobeConfig = mkIf cfg.victus-15-support.enable ''
              options hp-wmi force_fan_control_support=true
            '';
          };
        }
      );
    };
}
