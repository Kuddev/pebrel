{
  description = "Pebrel";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};

      version = (fromTOML (builtins.readFile ./nebula_app/Cargo.toml)).package.version;

      # 构建期依赖库，同时也是写进 rpath 的运行期库：GPUI/winit 会在运行时
      # dlopen 掉其中一部分（X11/Wayland/Vulkan），NixOS 上不写 rpath 会找不到。
      runtimeLibs =
        p:
        with p;
        [
          alsa-lib
          fontconfig
          freetype
          libGL
          libgit2
          libx11
          libxcb
          libxcursor
          libxext
          libxi
          libxkbcommon
          libxrandr
          openssl
          sqlite
          vulkan-loader
          wayland
          zlib
          zstd
        ];

      mkPebrel =
        p:
        let
          libs = runtimeLibs p;
        in
        p.rustPackages_1_98.rustPlatform.buildRustPackage {
          pname = "pebrel";
          inherit version;

          src = self;

          # 依赖变化（Cargo.lock 变更）后重算：把 cargoHash 设为 p.lib.fakeHash，
          # 运行 `nix build .#pebrel`，用报错里的 got 值更新这里。
          cargoHash = "sha256-bE++uF9kpMI66kKAIRlgd3t1E7FcqXECaYOh7Dnr/e4=";

          # 上游 README：cargo build --release --locked -p nebula --bin pebrel --features gpui-shell
          # 同时构建 AI Hook 辅助程序 pebrel-hook（nebula_hook）：应用在自身可执行
          # 文件同目录查找它（nebula_app/src/ai_hook/local.rs），scripts/package-linux.sh
          # 也要求它随应用安装；缺失会导致本地 Agent 集成无法安装。
          # 特性用 `nebula/gpui-shell` 限定，避免把 gpui-shell 施加到 nebula_hook。
          cargoBuildFlags = [
            "--locked"
            "--package=nebula"
            "--bin=pebrel"
            "--package=nebula_hook"
            "--bin=pebrel-hook"
          ];
          buildFeatures = [ "nebula/gpui-shell" ];

          nativeBuildInputs = with p; [
            cmake
            pkg-config
          ];
          buildInputs = libs;

          # 禁止各 -sys crate 走 vendored 源码下载，改链接系统库
          env = {
            OPENSSL_NO_VENDOR = "1";
            LIBGIT2_NO_VENDOR = "1";
            LIBSQLITE3_SYS_USE_PKG_CONFIG = "1";
            ZSTD_SYS_USE_PKG_CONFIG = "1";
          };

          # 上游测试需要 Xvfb 等图形环境，默认关闭
          doCheck = false;

          # 复刻上游 scripts/package-linux.sh 的 Linux 包布局
          postInstall = ''
            install -Dm644 packaging/linux/io.github.kuddev.pebrel.desktop \
              $out/share/applications/io.github.kuddev.pebrel.desktop
            install -Dm644 packaging/linux/io.github.kuddev.pebrel.metainfo.xml \
              $out/share/metainfo/io.github.kuddev.pebrel.metainfo.xml

            # 上游用 ImageMagick 把 1024x1024 的 nebula.png 缩成 256x256；这里不引入
            # imagemagick，改为把原图装到与其尺寸一致的 1024x1024 目录。
            # desktop 里 Icon=io.github.kuddev.pebrel 按名字查找，不绑定具体尺寸。
            install -Dm644 extra/logo/nebula.png \
              $out/share/icons/hicolor/1024x1024/apps/io.github.kuddev.pebrel.png

            install -Dm644 extra/completions/pebrel.bash $out/share/bash-completion/completions/pebrel
            install -Dm644 extra/completions/pebrel.fish $out/share/fish/vendor_completions.d/pebrel.fish
            install -Dm644 extra/completions/_pebrel $out/share/zsh/vendor-completions/_pebrel

            install -Dm644 LICENSE $out/share/doc/pebrel/licenses/LICENSE
            install -Dm644 THIRD-PARTY-NOTICES $out/share/doc/pebrel/licenses/THIRD-PARTY-NOTICES
            install -Dm644 licenses/LICENSE-LUA $out/share/doc/pebrel/licenses/LICENSE-LUA
            install -Dm644 licenses/LICENSE-MLUA $out/share/doc/pebrel/licenses/LICENSE-MLUA
            install -Dm644 licenses/LICENSE-LATIN-MODERN-MATH $out/share/doc/pebrel/licenses/LICENSE-LATIN-MODERN-MATH
          '';

          postFixup = ''
            patchelf $out/bin/pebrel --add-rpath ${p.lib.makeLibraryPath libs}
          '';

          meta = {
            description = "AI-native, GPU-accelerated terminal emulator";
            homepage = "https://github.com/Kuddev/pebrel";
            license = p.lib.licenses.gpl3Plus;
            mainProgram = "pebrel";
            platforms = [ "x86_64-linux" ];
          };
        };
    in
    {
      packages.${system} = rec {
        pebrel = mkPebrel pkgs;
        default = pebrel;
      };

      overlays.default = final: _prev: {
        pebrel = mkPebrel final;
      };
    };
}
