{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    bundlers = {
      url = "github:NixOS/bundlers";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    fenix,
    flake-utils,
    bundlers,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
        };
        lib = pkgs.lib;
        fenixPkgs = fenix.packages.${system};
        bundlePkgs = bundlers.bundlers.${system} or {};
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
        linuxStaticTarget = "x86_64-unknown-linux-musl";
        muslPkgs = pkgs.pkgsCross.musl64;
        windowsTarget = "x86_64-pc-windows-gnu";
        mingwPkgs = pkgs.pkgsCross.mingwW64;
        rlruDioxusAppId = "org.colonelpanic.rlru.dioxus";
        rlruDioxusDesktopAlias = "rlru-dioxus";
        toolchain = fenixPkgs.combine [
          fenixPkgs.stable.cargo
          fenixPkgs.stable.clippy
          fenixPkgs.stable.rust-src
          fenixPkgs.stable.rustc
          fenixPkgs.stable.rustfmt
          fenixPkgs.stable.rust-analyzer
          fenixPkgs.targets.wasm32-unknown-unknown.stable.rust-std
          fenixPkgs.targets.x86_64-unknown-linux-musl.stable.rust-std
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
          extraRustFlags ? [],
          desktopItems ? [],
          postInstall ? "",
          postFixup ? "",
        }:
          rustPlatform.buildRustPackage ({
              inherit pname buildFeatures buildNoDefaultFeatures;
              inherit desktopItems postInstall;
              version = "0.1.2";
              src = cleanSrc;
              cargoLock.lockFile = ./Cargo.lock;
              cargoBuildFlags = ["-p" cargoPackage];
              cargoTestFlags = ["-p" cargoPackage];
              nativeBuildInputs = [pkgs.pkg-config] ++ extraNativeBuildInputs;
              buildInputs = extraBuildInputs;
              RUST_MIN_STACK = "67108864";
              inherit postFixup;
              meta = {
                description = "Rocket League replay uploader";
                homepage = "https://github.com/rlrml/rlru";
                license = with lib.licenses; [mit asl20];
                mainProgram = pname;
                platforms = lib.platforms.unix ++ lib.platforms.windows;
              };
            }
            // lib.optionalAttrs (extraRustFlags != []) {
              RUSTFLAGS = lib.concatStringsSep " " extraRustFlags;
            });
        rlruLinuxStaticCli = rustPlatform.buildRustPackage {
          pname = "rlru";
          version = "0.1.2";
          src = cleanSrc;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = ["-p" "rlru" "--target" linuxStaticTarget];
          doCheck = false;
          nativeBuildInputs = [
            muslPkgs.stdenv.cc
          ];
          "CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER" = "${muslPkgs.stdenv.cc}/bin/${muslPkgs.stdenv.cc.targetPrefix}gcc";
          "CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_AR" = "${muslPkgs.stdenv.cc}/bin/${muslPkgs.stdenv.cc.targetPrefix}ar";
          "CC_x86_64_unknown_linux_musl" = "${muslPkgs.stdenv.cc}/bin/${muslPkgs.stdenv.cc.targetPrefix}gcc";
          "AR_x86_64_unknown_linux_musl" = "${muslPkgs.stdenv.cc}/bin/${muslPkgs.stdenv.cc.targetPrefix}ar";
          installPhase = ''
            runHook preInstall

            install -Dm755 "target/${linuxStaticTarget}/release/rlru" "$out/bin/rlru"

            runHook postInstall
          '';
          meta = {
            description = "Static Rocket League replay uploader CLI for Linux";
            homepage = "https://github.com/rlrml/rlru";
            license = with lib.licenses; [mit asl20];
            mainProgram = "rlru";
            platforms = lib.platforms.linux;
          };
        };
        mkWindowsPackage = {
          pname,
          cargoPackage ? "rlru",
        }:
          rustPlatform.buildRustPackage {
            inherit pname;
            version = "0.1.2";
            src = cleanSrc;
            cargoLock.lockFile = ./Cargo.lock;
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
            buildPhase = ''
              runHook preBuild

              cargo build --frozen --offline --release -p ${cargoPackage} --target ${windowsTarget}

              runHook postBuild
            '';
            installPhase = ''
              runHook preInstall

              install -Dm755 "target/${windowsTarget}/release/${pname}.exe" "$out/bin/${pname}.exe"

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

        packages =
          {
            default = mkRlruPackage {pname = "rlru";};
            rlru = mkRlruPackage {pname = "rlru";};
            rlru-linux-static = rlruLinuxStaticCli;
            rlru-windows = mkWindowsPackage {pname = "rlru";};
            rlru-dioxus-desktop = mkRlruPackage {
              pname = "rlru-dioxus";
              cargoPackage = "rlru-dioxus";
              buildNoDefaultFeatures = true;
              buildFeatures = ["desktop"];
              extraBuildInputs = dioxusLinuxBuildInputs ++ dioxusDarwinBuildInputs ++ [pkgs.openssl];
              extraNativeBuildInputs = lib.optionals isLinux [
                pkgs.copyDesktopItems
                pkgs.makeWrapper
              ];
              extraRustFlags = lib.optionals isLinux [
                "-C"
                "link-arg=-fuse-ld=bfd"
              ];
              desktopItems = lib.optionals isLinux [
                (pkgs.makeDesktopItem {
                  name = rlruDioxusAppId;
                  desktopName = "rlru";
                  genericName = "Rocket League replay uploader";
                  comment = "Upload Rocket League replay data";
                  exec = "rlru-dioxus";
                  icon = rlruDioxusAppId;
                  terminal = false;
                  categories = ["Game" "Utility"];
                  startupNotify = true;
                  startupWMClass = rlruDioxusAppId;
                })
                (pkgs.makeDesktopItem {
                  name = rlruDioxusDesktopAlias;
                  desktopName = "rlru";
                  genericName = "Rocket League replay uploader";
                  comment = "Upload Rocket League replay data";
                  exec = "rlru-dioxus";
                  icon = rlruDioxusDesktopAlias;
                  terminal = false;
                  categories = ["Game" "Utility"];
                  startupNotify = true;
                  startupWMClass = rlruDioxusDesktopAlias;
                })
              ];
              postInstall = lib.optionalString isLinux ''
                for size in 16 24 32 48 64 128 256 512 1024; do
                  for icon_name in ${rlruDioxusAppId} ${rlruDioxusDesktopAlias} rlru; do
                    install -Dm644 \
                      "crates/rlru-dioxus/assets/icons/rlru-icon-$size.png" \
                      "$out/share/icons/hicolor/''${size}x''${size}/apps/$icon_name.png"
                  done
                done
              '';
              postFixup = lib.optionalString isLinux ''
                wrapProgram $out/bin/rlru-dioxus \
                  --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath dioxusLinuxLibraryPathInputs} \
                  --prefix XDG_DATA_DIRS : ${dioxusLinuxGsettingsDataDirs} \
                  --set GIO_MODULE_DIR ${dioxusLinuxGioModuleDir} \
                  --set-default WEBKIT_DISABLE_DMABUF_RENDERER 1
              '';
            };
            dist-cli-linux-x86_64 =
              pkgs.runCommand "rlru-cli-linux-x86_64.tar.gz" {
                nativeBuildInputs = [
                  pkgs.gnutar
                  pkgs.gzip
                ];
              } ''
                package="rlru-cli-linux-x86_64"
                  mkdir -p "$TMPDIR/$package/bin"
                  cp "${self.packages.${system}.rlru-linux-static}/bin/rlru" "$TMPDIR/$package/bin/rlru"
                  chmod +x "$TMPDIR/$package/bin/"*
                  tar -C "$TMPDIR" -czf "$out" "$package"
              '';
            dist-cli-windows-x86_64 =
              pkgs.runCommand "rlru-cli-windows-x86_64.zip" {
                nativeBuildInputs = [
                  pkgs.zip
                ];
              } ''
                package="rlru-cli-windows-x86_64"
                  mkdir -p "$TMPDIR/$package"
                  cp "${self.packages.${system}.rlru-windows}/bin/rlru.exe" "$TMPDIR/$package/"
                  (cd "$TMPDIR" && zip -r "$out" "$package")
              '';
          }
          // lib.optionalAttrs isLinux {
            rlru-dioxus-appimage = bundlePkgs.toAppImage self.packages.${system}.rlru-dioxus-desktop;
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
