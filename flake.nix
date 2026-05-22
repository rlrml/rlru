{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    fenix,
    flake-utils,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };
        lib = pkgs.lib;
        fenixPkgs = fenix.packages.${system};
        sourceRoot = ./.;
        cleanSrc = lib.cleanSourceWith {
          src = sourceRoot;
          filter = path: type: let
            pathStr = toString path;
            rootStr = toString sourceRoot;
            rel =
              if pathStr == rootStr
              then ""
              else lib.removePrefix "${rootStr}/" pathStr;
            base = builtins.baseNameOf pathStr;
          in
            !(builtins.elem base [
              ".git"
              ".direnv"
              ".worktrees"
              "target"
              "dist"
              "result"
            ])
            && !(lib.hasPrefix ".worktrees/" rel);
        };
        isLinux = pkgs.stdenv.hostPlatform.isLinux;
        isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
        windowsTarget = "x86_64-pc-windows-gnu";
        mingwPkgs = pkgs.pkgsCross.mingwW64;
        toolchain = fenixPkgs.combine [
          fenixPkgs.stable.cargo
          fenixPkgs.stable.clippy
          fenixPkgs.stable.rust-src
          fenixPkgs.stable.rustc
          fenixPkgs.stable.rustfmt
          fenixPkgs.stable.rust-analyzer
          fenixPkgs.targets.wasm32-unknown-unknown.stable.rust-std
          fenixPkgs.targets.x86_64-pc-windows-gnu.stable.rust-std
        ];
        rustPlatform = pkgs.makeRustPlatform {
          cargo = toolchain;
          rustc = toolchain;
        };
        dioxusLinuxBuildInputs = lib.optionals isLinux [
          pkgs.dbus
          pkgs.glib-networking
          pkgs.gsettings-desktop-schemas
          pkgs.glib
          pkgs.gtk3
          pkgs.libappindicator-gtk3
          pkgs.webkitgtk_4_1
          pkgs.xdotool
        ];
        dioxusLinuxLibraryPathInputs = lib.optionals isLinux [
          pkgs.cairo
          pkgs.gdk-pixbuf
          pkgs.glib
          pkgs.gtk3
          pkgs.harfbuzz
          pkgs.libappindicator-gtk3
          pkgs.libsoup_3
          pkgs.openssl
          pkgs.pango
          pkgs.webkitgtk_4_1
          pkgs.xdotool
          pkgs.zlib
        ];
        dioxusLinuxGsettingsDataDirs = lib.optionalString isLinux (lib.concatStringsSep ":" [
          "${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}"
          "${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}"
        ]);
        dioxusLinuxGioModuleDir = lib.optionalString isLinux "${pkgs.glib-networking}/lib/gio/modules";
        dioxusDarwinBuildInputs = lib.optionals isDarwin [
          pkgs.apple-sdk_15
        ];
        mkRlruPackage = {
          pname,
          cargoPackage ? "rlru",
          buildFeatures ? [],
          buildNoDefaultFeatures ? false,
          extraBuildInputs ? [],
          extraNativeBuildInputs ? [],
          postFixup ? "",
        }:
          rustPlatform.buildRustPackage {
            inherit pname buildFeatures buildNoDefaultFeatures;
            version = "0.1.0";
            src = cleanSrc;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = ["-p" cargoPackage];
            cargoTestFlags = ["-p" cargoPackage];
            nativeBuildInputs = [pkgs.pkg-config] ++ extraNativeBuildInputs;
            buildInputs = extraBuildInputs;
            inherit postFixup;
            meta = {
              description = "Rocket League replay uploader";
              homepage = "https://github.com/rlrml/rlru";
              license = with lib.licenses; [mit asl20];
              mainProgram = pname;
              platforms = lib.platforms.unix ++ lib.platforms.windows;
            };
          };
        mkWindowsPackage = {
          pname,
          cargoPackage ? "rlru",
          buildFeatures ? [],
          buildNoDefaultFeatures ? false,
          installExtra ? "",
        }:
          rustPlatform.buildRustPackage {
            inherit pname buildFeatures buildNoDefaultFeatures;
            version = "0.1.0";
            src = cleanSrc;
            cargoLock.lockFile = ./Cargo.lock;
            cargoBuildFlags = ["-p" cargoPackage "--target" windowsTarget];
            doCheck = false;
            nativeBuildInputs = [
              pkgs.pkg-config
              mingwPkgs.stdenv.cc
            ];
            buildInputs = [
              mingwPkgs.windows.pthreads
            ];
            "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER" = "${mingwPkgs.stdenv.cc}/bin/${mingwPkgs.stdenv.cc.targetPrefix}gcc";
            "CARGO_TARGET_X86_64_PC_WINDOWS_GNU_AR" = "${mingwPkgs.stdenv.cc}/bin/${mingwPkgs.stdenv.cc.targetPrefix}ar";
            "CC_x86_64_pc_windows_gnu" = "${mingwPkgs.stdenv.cc}/bin/${mingwPkgs.stdenv.cc.targetPrefix}gcc";
            "AR_x86_64_pc_windows_gnu" = "${mingwPkgs.stdenv.cc}/bin/${mingwPkgs.stdenv.cc.targetPrefix}ar";
            installPhase = ''
              runHook preInstall

              install -Dm755 "target/${windowsTarget}/release/${pname}.exe" "$out/bin/${pname}.exe"
              ${installExtra}

              runHook postInstall
            '';
            meta = {
              description = "Rocket League replay uploader for Windows";
              homepage = "https://github.com/rlrml/rlru";
              license = with lib.licenses; [mit asl20];
              mainProgram = "${pname}.exe";
              platforms = lib.platforms.all;
            };
          };
      in {
        formatter = pkgs.alejandra;

        packages = {
          default = mkRlruPackage {pname = "rlru";};
          rlru = mkRlruPackage {pname = "rlru";};
          rlru-windows = mkWindowsPackage {pname = "rlru";};
          rlru-dioxus-desktop = mkRlruPackage {
            pname = "rlru-dioxus";
            cargoPackage = "rlru-dioxus";
            buildNoDefaultFeatures = true;
            buildFeatures = ["desktop"];
            extraBuildInputs = dioxusLinuxBuildInputs ++ dioxusDarwinBuildInputs ++ [pkgs.openssl];
            extraNativeBuildInputs = lib.optionals isLinux [pkgs.makeWrapper];
            postFixup = lib.optionalString isLinux ''
              wrapProgram $out/bin/rlru-dioxus \
                --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath dioxusLinuxLibraryPathInputs} \
                --prefix XDG_DATA_DIRS : ${dioxusLinuxGsettingsDataDirs} \
                --set GIO_MODULE_DIR ${dioxusLinuxGioModuleDir} \
                --set-default WEBKIT_DISABLE_DMABUF_RENDERER 1
            '';
          };
          rlru-dioxus-windows = mkWindowsPackage {
            pname = "rlru-dioxus";
            cargoPackage = "rlru-dioxus";
            buildNoDefaultFeatures = true;
            buildFeatures = ["desktop"];
            installExtra = ''
              loader="$(find "target/${windowsTarget}/release/build" -path '*/out/x64/WebView2Loader.dll' -print -quit || true)"
              if [[ -n "$loader" ]]; then
                install -Dm644 "$loader" "$out/bin/WebView2Loader.dll"
              fi
            '';
          };
          dist-linux-x86_64 =
            pkgs.runCommand "rlru-linux-x86_64.tar.gz" {
              nativeBuildInputs = [
                pkgs.gnutar
                pkgs.gzip
              ];
            } ''
              package="rlru-linux-x86_64"
              mkdir -p "$TMPDIR/$package/bin"
              cp "${self.packages.${system}.rlru}/bin/rlru" "$TMPDIR/$package/bin/rlru"
              if [[ -x "${self.packages.${system}.rlru-dioxus-desktop}/bin/.rlru-dioxus-wrapped" ]]; then
                cp "${self.packages.${system}.rlru-dioxus-desktop}/bin/.rlru-dioxus-wrapped" "$TMPDIR/$package/bin/rlru-dioxus"
              else
                cp "${self.packages.${system}.rlru-dioxus-desktop}/bin/rlru-dioxus" "$TMPDIR/$package/bin/rlru-dioxus"
              fi
              chmod +x "$TMPDIR/$package/bin/"*
              tar -C "$TMPDIR" -czf "$out" "$package"
            '';
          dist-windows-x86_64 =
            pkgs.runCommand "rlru-windows-x86_64.zip" {
              nativeBuildInputs = [
                pkgs.zip
              ];
            } ''
              package="rlru-windows-x86_64"
              mkdir -p "$TMPDIR/$package"
              cp "${self.packages.${system}.rlru-windows}/bin/rlru.exe" "$TMPDIR/$package/"
              cp "${self.packages.${system}.rlru-dioxus-windows}/bin/rlru-dioxus.exe" "$TMPDIR/$package/"
              if [[ -f "${self.packages.${system}.rlru-dioxus-windows}/bin/WebView2Loader.dll" ]]; then
                cp "${self.packages.${system}.rlru-dioxus-windows}/bin/WebView2Loader.dll" "$TMPDIR/$package/"
              fi
              (cd "$TMPDIR" && zip -r "$out" "$package")
            '';
        };

        apps = {
          default = {
            type = "app";
            program = "${self.packages.${system}.rlru}/bin/rlru";
          };
          rlru = {
            type = "app";
            program = "${self.packages.${system}.rlru}/bin/rlru";
          };
          rlru-dioxus-desktop = {
            type = "app";
            program = "${self.packages.${system}.rlru-dioxus-desktop}/bin/rlru-dioxus";
          };
        };

        devShells.default = pkgs.mkShell {
          buildInputs =
            [
              toolchain
              pkgs.binaryen
              pkgs.dioxus-cli
              pkgs.just
              pkgs.pkgsCross.mingwW64.stdenv.cc
              pkgs.pkgsCross.mingwW64.windows.pthreads
              pkgs.openssl
              pkgs.pkg-config
              pkgs.wasm-bindgen-cli
              pkgs.zip
            ]
            ++ dioxusLinuxBuildInputs;

          LD_LIBRARY_PATH = lib.optionalString isLinux (lib.makeLibraryPath dioxusLinuxLibraryPathInputs);
          WEBKIT_DISABLE_DMABUF_RENDERER = "1";
          shellHook = lib.optionalString isLinux ''
            export XDG_DATA_DIRS="${dioxusLinuxGsettingsDataDirs}:''${XDG_DATA_DIRS:-}"
            export GIO_MODULE_DIR="${dioxusLinuxGioModuleDir}"
          '';
        };
      }
    )
    // {
      nixosModules.default = import ./nix/nixos-module.nix self;
      nixosModules.rlru = self.nixosModules.default;

      homeManagerModules.default = import ./nix/home-manager-module.nix self;
      homeManagerModules.rlru = self.homeManagerModules.default;
    };
}
