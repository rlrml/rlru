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
          config = {
            allowUnfree = true;
            android_sdk.accept_license = true;
          };
        };
        lib = pkgs.lib;
        fenixPkgs = fenix.packages.${system};
        bundlePkgs = bundlers.bundlers.${system} or {};
        # Git commit for `--version` / the GUI About view. The build scripts
        # cannot read `.git` (it is stripped from `cleanSrc`), so surface the
        # flake's revision through an env var instead. Falls back to the dirty
        # revision when the tree is uncommitted, then to "unknown".
        rlruGitCommit = self.rev or self.dirtyRev or "unknown";
        packageVersion = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
        androidVersionCode = "113";
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
        androidRustTargets = lib.optionals isLinux [
          fenixPkgs.targets.aarch64-linux-android.stable.rust-std
          fenixPkgs.targets.x86_64-linux-android.stable.rust-std
        ];
        toolchain =
          fenixPkgs.combine
          ([
              fenixPkgs.stable.cargo
              fenixPkgs.stable.clippy
              fenixPkgs.stable.rust-src
              fenixPkgs.stable.rustc
              fenixPkgs.stable.rustfmt
              fenixPkgs.stable.rust-analyzer
              fenixPkgs.targets.wasm32-unknown-unknown.stable.rust-std
              fenixPkgs.targets.x86_64-unknown-linux-musl.stable.rust-std
              fenixPkgs.targets.x86_64-pc-windows-gnu.stable.rust-std
            ]
            ++ androidRustTargets);
        rustPlatform = pkgs.makeRustPlatform {
          cargo = toolchain;
          rustc = toolchain;
        };
        androidBuildToolsVersion = "36.1.0";
        androidCmdLineToolsVersion = "19.0";
        androidCompileSdkVersion = "36";
        androidGradlePluginVersion = "8.13.2";
        androidKotlinPluginVersion = "2.2.21";
        androidNdkVersion = "29.0.14206865";
        androidPlatformToolsVersion = "36.0.2";
        androidTargetSdkVersion = "36";
        androidComposition = pkgs.androidenv.composeAndroidPackages {
          cmdLineToolsVersion = androidCmdLineToolsVersion;
          toolsVersion = "26.1.1";
          platformToolsVersion = androidPlatformToolsVersion;
          buildToolsVersions = ["34.0.0" androidBuildToolsVersion];
          includeEmulator = true;
          platformVersions = ["33" "34" androidCompileSdkVersion];
          includeSources = false;
          includeSystemImages = false;
          systemImageTypes = ["google_apis_playstore"];
          abiVersions = ["arm64-v8a" "x86_64"];
          includeNDK = true;
          ndkVersions = [androidNdkVersion];
          cmakeVersions = ["3.22.1"];
          useGoogleAPIs = true;
          useGoogleTVAddOns = false;
        };
        androidHome = "${androidComposition.androidsdk}/libexec/android-sdk";
        androidNdkHome = "${androidHome}/ndk/${androidNdkVersion}";
        androidAapt2 = "${androidHome}/build-tools/${androidBuildToolsVersion}/aapt2";
        androidLlvmBin = "${androidNdkHome}/toolchains/llvm/prebuilt/linux-x86_64/bin";
        android16KbPageRustFlags = "-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384";
        dioxusAndroidEnv = {
          ANDROID_HOME = androidHome;
          ANDROID_SDK_ROOT = androidHome;
          ANDROID_NDK_HOME = androidNdkHome;
          NDK_HOME = androidNdkHome;
          CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = "${androidLlvmBin}/aarch64-linux-android24-clang";
          CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS = android16KbPageRustFlags;
          CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER = "${androidLlvmBin}/x86_64-linux-android24-clang";
          CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS = android16KbPageRustFlags;
          CC_aarch64_linux_android = "${androidLlvmBin}/aarch64-linux-android24-clang";
          CC_x86_64_linux_android = "${androidLlvmBin}/x86_64-linux-android24-clang";
          AR_aarch64_linux_android = "${androidLlvmBin}/llvm-ar";
          AR_x86_64_linux_android = "${androidLlvmBin}/llvm-ar";
          GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${androidAapt2}";
          JAVA_HOME = pkgs.jdk17.home;
          OPENSSL_NO_VENDOR = "0";
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
              version = packageVersion;
              src = cleanSrc;
              cargoLock.lockFile = ./Cargo.lock;
              cargoBuildFlags = ["-p" cargoPackage];
              cargoTestFlags = ["-p" cargoPackage];
              nativeBuildInputs = [pkgs.pkg-config] ++ extraNativeBuildInputs;
              buildInputs = extraBuildInputs;
              RUST_MIN_STACK = "536870912";
              RLRU_GIT_COMMIT = rlruGitCommit;
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
          version = packageVersion;
          src = cleanSrc;
          cargoLock.lockFile = ./Cargo.lock;
          cargoBuildFlags = ["-p" "rlru" "--target" linuxStaticTarget];
          doCheck = false;
          RLRU_GIT_COMMIT = rlruGitCommit;
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
            version = packageVersion;
            src = cleanSrc;
            cargoLock.lockFile = ./Cargo.lock;
            doCheck = false;
            RLRU_GIT_COMMIT = rlruGitCommit;
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
        dioxusAndroidBuildScript = release:
          pkgs.writeShellApplication {
            name = "rlru-dioxus-android-${
              if release
              then "release"
              else "debug"
            }";
            runtimeInputs = [
              pkgs.coreutils
              pkgs.findutils
              pkgs.gnused
              pkgs.imagemagick
              pkgs.jq
              pkgs.nix
            ];
            text = ''
              set -euo pipefail

              repo="''${RLRU_ROOT:-$PWD}"
              cd "$repo"

              profile=${
                if release
                then "release"
                else "debug"
              }

              args=(
                dx ${
                if release
                then "bundle"
                else "build"
              } --android
                --target aarch64-linux-android
                --package rlru-dioxus
                --no-default-features
                --features android
              )

              if ${
                if release
                then "true"
                else "false"
              }; then
                args+=(--release)
              fi

              patch_android_project() {
                local gradle_root="target/dx/rlru-dioxus/$profile/android/app"
                local root_gradle="$gradle_root/build.gradle.kts"
                local app_gradle="$gradle_root/app/build.gradle.kts"
                local gradle_properties="$gradle_root/gradle.properties"
                local manifest="$gradle_root/app/src/main/AndroidManifest.xml"
                local res="$gradle_root/app/src/main/res"

                if [[ -f "$root_gradle" ]]; then
                  sed -i \
                    -e 's/com\.android\.tools\.build:gradle:[^"]*/com.android.tools.build:gradle:${androidGradlePluginVersion}/' \
                    -e 's/org\.jetbrains\.kotlin:kotlin-gradle-plugin:[^"]*/org.jetbrains.kotlin:kotlin-gradle-plugin:${androidKotlinPluginVersion}/' \
                    "$root_gradle"
                fi

                if [[ -f "$app_gradle" ]]; then
                  sed -i \
                    -e 's/compileSdk = [0-9][0-9]*/compileSdk = ${androidCompileSdkVersion}\n    buildToolsVersion = "${androidBuildToolsVersion}"/' \
                    -e 's/targetSdk = [0-9][0-9]*/targetSdk = ${androidTargetSdkVersion}/' \
                    -e 's/versionCode = [0-9][0-9]*/versionCode = ${androidVersionCode}/' \
                    -e 's/versionName = "[^"]*"/versionName = "${packageVersion}"/' \
                    -e '/^[[:space:]]*kotlinOptions[[:space:]]*{/,/^[[:space:]]*}/c\    kotlin {\n        compilerOptions {\n            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)\n        }\n    }' \
                    "$app_gradle"
                fi

                if [[ -f "$gradle_properties" ]]; then
                  sed -i '/^android\.defaults\.buildfeatures\.buildconfig=/d' "$gradle_properties"
                fi

                if [[ -f "$manifest" ]]; then
                  sed -i '/android:extractNativeLibs=/d' "$manifest"
                fi

                if [[ -d "$res" ]]; then
                  rm -f "$res/mipmap-anydpi-v26/ic_launcher.xml"
                  for density in mdpi:48 hdpi:72 xhdpi:96 xxhdpi:144 xxxhdpi:192; do
                    local qualifier="''${density%%:*}"
                    local size="''${density##*:}"
                    local dir="$res/mipmap-$qualifier"
                    mkdir -p "$dir"
                    rm -f "$dir/ic_launcher.webp"
                    magick -background none "$repo/crates/rlru-dioxus/assets/icons/rlru-icon-1024.png" \
                      -alpha set -resize "''${size}x''${size}" \
                      "$dir/ic_launcher.png"
                  done
                fi
              }

              sign_release_apks() {
                if [[ -z "''${ANDROID_SIGNING_KEYSTORE_BASE64:-}" && -z "''${ANDROID_SIGNING_KEYSTORE_FILE:-}" ]]; then
                  echo "Android release signing skipped: no signing keystore was provided"
                  return
                fi

                local required=(
                  ANDROID_SIGNING_KEY_ALIAS
                  ANDROID_SIGNING_KEYSTORE_PASSWORD
                  ANDROID_SIGNING_KEY_PASSWORD
                )

                for var in "''${required[@]}"; do
                  if [[ -z "''${!var:-}" ]]; then
                    echo "Android release signing requires $var" >&2
                    exit 1
                  fi
                done

                local signing_dir
                signing_dir="$(mktemp -d)"
                trap 'rm -rf "$signing_dir"' RETURN

                local keystore="$signing_dir/rlru-release.keystore"
                if [[ -n "''${ANDROID_SIGNING_KEYSTORE_FILE:-}" ]]; then
                  cp "$ANDROID_SIGNING_KEYSTORE_FILE" "$keystore"
                else
                  printf '%s' "$ANDROID_SIGNING_KEYSTORE_BASE64" | base64 -d > "$keystore"
                fi

                local apk_dir="target/dx/rlru-dioxus/release/android/app/app/build/outputs/apk/release"
                shopt -s nullglob
                local unsigned_apks=("$apk_dir"/*-unsigned.apk)
                shopt -u nullglob

                if ((''${#unsigned_apks[@]} == 0)); then
                  echo "No unsigned release APKs were found to sign in $apk_dir" >&2
                  exit 1
                fi

                for unsigned_apk in "''${unsigned_apks[@]}"; do
                  local apk_base="''${unsigned_apk%-unsigned.apk}"
                  local aligned_apk
                  aligned_apk="$signing_dir/$(basename "$apk_base")-aligned.apk"
                  local signed_apk="$apk_base-signed.apk"

                  # shellcheck disable=SC2016
                  nix develop "$repo#android" --command bash -lc '
                    set -euo pipefail
                    unsigned_apk="$1"
                    aligned_apk="$2"
                    signed_apk="$3"
                    keystore="$4"
                    key_alias="$5"
                    storepass="$6"
                    keypass="$7"

                    "$ANDROID_HOME/build-tools/${androidBuildToolsVersion}/zipalign" -p -f 4 "$unsigned_apk" "$aligned_apk"
                    "$ANDROID_HOME/build-tools/${androidBuildToolsVersion}/apksigner" sign \
                      --ks "$keystore" \
                      --ks-key-alias "$key_alias" \
                      --ks-pass "pass:$storepass" \
                      --key-pass "pass:$keypass" \
                      --out "$signed_apk" \
                      "$aligned_apk"
                    "$ANDROID_HOME/build-tools/${androidBuildToolsVersion}/apksigner" verify --verbose "$signed_apk"
                  ' bash \
                    "$unsigned_apk" \
                    "$aligned_apk" \
                    "$signed_apk" \
                    "$keystore" \
                    "$ANDROID_SIGNING_KEY_ALIAS" \
                    "$ANDROID_SIGNING_KEYSTORE_PASSWORD" \
                    "$ANDROID_SIGNING_KEY_PASSWORD"
                done
              }

              rm -rf "target/dx/rlru-dioxus/$profile/android"
              nix develop "$repo#android" --command "''${args[@]}" "$@"
              patch_android_project

              if ${
                if release
                then "true"
                else "false"
              }; then
                nix develop "$repo#android" --command bash -lc \
                  'cd target/dx/rlru-dioxus/release/android/app && ./gradlew :app:bundleRelease :app:assembleRelease --no-daemon --console plain'
                sign_release_apks
              else
                nix develop "$repo#android" --command bash -lc \
                  'cd target/dx/rlru-dioxus/debug/android/app && ./gradlew :app:assembleDebug --no-daemon --console plain'
              fi

              if ${
                if release
                then "true"
                else "false"
              }; then
                find "$repo/target/dx/rlru-dioxus/release/android" \
                  \( -path '*/build/outputs/apk/release/*.apk' -o -path '*/build/outputs/bundle/release/*.aab' \) \
                  -print
              else
                find "$repo/target/dx/rlru-dioxus/$profile/android" \
                  -path '*/build/outputs/apk/debug/*.apk' \
                  -print
              fi
            '';
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
            rlru-dioxus-android-debug-runner = dioxusAndroidBuildScript false;
            rlru-dioxus-android-release-runner = dioxusAndroidBuildScript true;
          };

        apps =
          {
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
          }
          // lib.optionalAttrs isLinux {
            dioxus-android-debug = {
              type = "app";
              program = "${dioxusAndroidBuildScript false}/bin/rlru-dioxus-android-debug";
            };
            dioxus-android-release = {
              type = "app";
              program = "${dioxusAndroidBuildScript true}/bin/rlru-dioxus-android-release";
            };
            rlru-dioxus-android-debug = {
              type = "app";
              program = "${dioxusAndroidBuildScript false}/bin/rlru-dioxus-android-debug";
            };
            rlru-dioxus-android-release = {
              type = "app";
              program = "${dioxusAndroidBuildScript true}/bin/rlru-dioxus-android-release";
            };
          };

        devShells = {
          default = pkgs.mkShell {
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
          android = pkgs.mkShell (dioxusAndroidEnv
            // {
              buildInputs =
                [
                  toolchain
                  pkgs.binaryen
                  pkgs.dioxus-cli
                  pkgs.gradle_9
                  pkgs.jdk17
                  pkgs.jq
                  pkgs.just
                  pkgs.openssl
                  pkgs.pkg-config
                  pkgs.wasm-bindgen-cli
                ]
                ++ dioxusLinuxBuildInputs;

              LD_LIBRARY_PATH = lib.optionalString isLinux (lib.makeLibraryPath dioxusLinuxLibraryPathInputs);
              WEBKIT_DISABLE_DMABUF_RENDERER = "1";
              shellHook = ''
                export PATH=${androidHome}/emulator:${androidHome}/platform-tools:${androidHome}/cmdline-tools/${androidCmdLineToolsVersion}/bin:$PATH

                echo "rlru Dioxus Android dev shell"
                echo "  dx: $(dx --version)"
                echo "  ANDROID_HOME: $ANDROID_HOME"
                echo ""
                echo "Commands:"
                echo "  just dioxus-android-build"
                echo "  just dioxus-android-release"
                echo "  nix run .#dioxus-android-release"
              '';
            });
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
