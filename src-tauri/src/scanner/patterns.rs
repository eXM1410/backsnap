//! Phase 1: Known exclude patterns.
//!
//! This is the authoritative table of paths where we KNOW the content
//! is regenerable (caches, toolchains, game files, etc.).

use super::types::ExcludeCategory;

/// A known exclude pattern entry.
///
/// - `check_path`: path to test for existence (may contain `*` for glob).
/// - `category`: classification for the UI.
/// - `reason`: human-readable explanation (German).
/// - `exclude_paths`: actual paths to exclude; if empty, uses `check_path`.
pub struct KnownPattern {
    pub check_path: &'static str,
    pub category: ExcludeCategory,
    pub reason: &'static str,
    pub exclude_paths: &'static [&'static str],
}

/// All known exclude patterns. If `exclude_paths` is empty the `check_path` itself is excluded.
pub static KNOWN_PATTERNS: &[KnownPattern] = &[
    // ─── Universal Caches & Trash ─────────────────────────
    KnownPattern {
        check_path: ".cache",
        category: ExcludeCategory::Cache,
        reason: "Anwendungs-Caches — alle Apps regenerieren diese automatisch",
        exclude_paths: &[".cache"],
    },
    KnownPattern {
        check_path: ".local/share/Trash",
        category: ExcludeCategory::Cache,
        reason: "Papierkorb — gelöschte Dateien",
        exclude_paths: &[".local/share/Trash"],
    },
    KnownPattern {
        check_path: ".thumbnails",
        category: ExcludeCategory::Cache,
        reason: "Vorschaubilder (werden neu generiert)",
        exclude_paths: &[".thumbnails"],
    },
    KnownPattern {
        check_path: ".local/share/recently-used.xbel",
        category: ExcludeCategory::Cache,
        reason: "Zuletzt verwendete Dateien (wird neu aufgebaut)",
        exclude_paths: &[".local/share/recently-used.xbel"],
    },
    // ─── Desktop Environment Caches ───────────────────────
    KnownPattern {
        check_path: ".local/share/baloo",
        category: ExcludeCategory::Cache,
        reason: "KDE Baloo Datei-Indexer (wird automatisch neu aufgebaut)",
        exclude_paths: &[".local/share/baloo"],
    },
    KnownPattern {
        check_path: ".local/share/tracker",
        category: ExcludeCategory::Cache,
        reason: "GNOME Tracker Datei-Indexer",
        exclude_paths: &[".local/share/tracker", ".local/share/tracker3"],
    },
    KnownPattern {
        check_path: ".local/share/akonadi",
        category: ExcludeCategory::Cache,
        reason: "KDE Akonadi PIM-Cache (wird von KMail/Kontact neu aufgebaut)",
        exclude_paths: &[".local/share/akonadi"],
    },
    KnownPattern {
        check_path: ".local/share/klipper",
        category: ExcludeCategory::Cache,
        reason: "KDE Klipper Zwischenablage-Verlauf",
        exclude_paths: &[".local/share/klipper"],
    },
    // ─── Browser Caches ───────────────────────────────────
    KnownPattern {
        check_path: ".mozilla/firefox/*/cache2",
        category: ExcludeCategory::Browser,
        reason: "Firefox Cache & Crash Reports",
        exclude_paths: &[
            ".mozilla/firefox/*/cache2",
            ".mozilla/firefox/Crash Reports",
        ],
    },
    KnownPattern {
        check_path: ".config/google-chrome/Default/Service Worker",
        category: ExcludeCategory::Browser,
        reason: "Chrome Service Worker & Shader Cache",
        exclude_paths: &[
            ".config/google-chrome/Default/Service Worker",
            ".config/google-chrome/Default/GPUCache",
            ".config/google-chrome/ShaderCache",
            ".config/google-chrome/Crash Reports",
        ],
    },
    KnownPattern {
        check_path: ".config/chromium/Default/Service Worker",
        category: ExcludeCategory::Browser,
        reason: "Chromium Service Worker & Cache",
        exclude_paths: &[
            ".config/chromium/Default/Service Worker",
            ".config/chromium/Default/GPUCache",
            ".config/chromium/ShaderCache",
            ".config/chromium/Crash Reports",
        ],
    },
    KnownPattern {
        check_path: ".config/BraveSoftware/Brave-Browser/Default/Service Worker",
        category: ExcludeCategory::Browser,
        reason: "Brave Browser Cache",
        exclude_paths: &[
            ".config/BraveSoftware/Brave-Browser/Default/Service Worker",
            ".config/BraveSoftware/Brave-Browser/Default/GPUCache",
            ".config/BraveSoftware/Brave-Browser/ShaderCache",
        ],
    },
    KnownPattern {
        check_path: ".config/vivaldi/Default/Service Worker",
        category: ExcludeCategory::Browser,
        reason: "Vivaldi Browser Cache",
        exclude_paths: &[
            ".config/vivaldi/Default/Service Worker",
            ".config/vivaldi/Default/GPUCache",
        ],
    },
    // ─── Communication App Caches ─────────────────────────
    KnownPattern {
        check_path: ".config/discord/Cache",
        category: ExcludeCategory::Communication,
        reason: "Discord Cache (wird automatisch neu geladen)",
        exclude_paths: &[
            ".config/discord/Cache",
            ".config/discord/Code Cache",
            ".config/discord/GPUCache",
        ],
    },
    KnownPattern {
        check_path: ".config/Slack/Cache",
        category: ExcludeCategory::Communication,
        reason: "Slack Cache",
        exclude_paths: &[
            ".config/Slack/Cache",
            ".config/Slack/Code Cache",
            ".config/Slack/GPUCache",
        ],
    },
    KnownPattern {
        check_path: ".config/Signal/attachments.noindex",
        category: ExcludeCategory::Communication,
        reason: "Signal heruntergeladene Anhänge (werden vom Server neu geladen)",
        exclude_paths: &[".config/Signal/attachments.noindex"],
    },
    KnownPattern {
        check_path: ".config/Microsoft/Microsoft Teams/Cache",
        category: ExcludeCategory::Communication,
        reason: "Teams Cache",
        exclude_paths: &[
            ".config/Microsoft/Microsoft Teams/Cache",
            ".config/Microsoft/Microsoft Teams/GPUCache",
        ],
    },
    KnownPattern {
        check_path: ".local/share/TelegramDesktop/tdata/user_data",
        category: ExcludeCategory::Communication,
        reason: "Telegram heruntergeladene Medien",
        exclude_paths: &[".local/share/TelegramDesktop/tdata/user_data"],
    },
    // ─── Node.js / JavaScript ─────────────────────────────
    KnownPattern {
        check_path: ".npm",
        category: ExcludeCategory::Cache,
        reason: "npm-Paket-Cache — npm install regeneriert alles",
        exclude_paths: &[".npm"],
    },
    KnownPattern {
        check_path: ".pnpm-store",
        category: ExcludeCategory::Cache,
        reason: "pnpm Content-Addressable Store",
        exclude_paths: &[".pnpm-store"],
    },
    KnownPattern {
        check_path: ".yarn/cache",
        category: ExcludeCategory::Cache,
        reason: "Yarn Berry-Cache",
        exclude_paths: &[".yarn/cache"],
    },
    KnownPattern {
        check_path: ".bun/install/cache",
        category: ExcludeCategory::Cache,
        reason: "Bun-Paket-Cache",
        exclude_paths: &[".bun/install/cache"],
    },
    KnownPattern {
        check_path: ".deno",
        category: ExcludeCategory::Cache,
        reason: "Deno-Cache (Module, Kompilate)",
        exclude_paths: &[".deno"],
    },
    KnownPattern {
        check_path: ".nvm/versions",
        category: ExcludeCategory::Toolchain,
        reason: "nvm Node.js-Versionen (nvm install regeneriert)",
        exclude_paths: &[".nvm/versions"],
    },
    KnownPattern {
        check_path: ".fnm/node-versions",
        category: ExcludeCategory::Toolchain,
        reason: "fnm Node.js-Versionen",
        exclude_paths: &[".fnm/node-versions"],
    },
    KnownPattern {
        check_path: ".volta/tools",
        category: ExcludeCategory::Toolchain,
        reason: "Volta Node.js Toolchain",
        exclude_paths: &[".volta/tools"],
    },
    // ─── Rust ─────────────────────────────────────────────
    KnownPattern {
        check_path: ".cargo/registry",
        category: ExcludeCategory::Cache,
        reason: "Cargo crate registry (cargo build regeneriert)",
        exclude_paths: &[".cargo/registry", ".cargo/git"],
    },
    KnownPattern {
        check_path: ".rustup/toolchains",
        category: ExcludeCategory::Toolchain,
        reason: "Rust-Toolchains (rustup install regeneriert)",
        exclude_paths: &[".rustup/toolchains", ".rustup/tmp"],
    },
    // ─── Python ───────────────────────────────────────────
    KnownPattern {
        check_path: ".local/lib/python*/site-packages",
        category: ExcludeCategory::Cache,
        reason: "Python user site-packages (pip install regeneriert)",
        exclude_paths: &[".local/lib"],
    },
    KnownPattern {
        check_path: ".local/share/pip",
        category: ExcludeCategory::Cache,
        reason: "pip Download-Cache",
        exclude_paths: &[".local/share/pip"],
    },
    KnownPattern {
        check_path: ".local/share/virtualenvs",
        category: ExcludeCategory::Cache,
        reason: "Pipenv Virtual Environments",
        exclude_paths: &[".local/share/virtualenvs"],
    },
    KnownPattern {
        check_path: ".cache/pypoetry",
        category: ExcludeCategory::Cache,
        reason: "Poetry-Cache",
        exclude_paths: &[".cache/pypoetry"],
    },
    KnownPattern {
        check_path: ".conda/pkgs",
        category: ExcludeCategory::Cache,
        reason: "Conda-Paket-Cache",
        exclude_paths: &[".conda/pkgs"],
    },
    KnownPattern {
        check_path: ".local/share/uv",
        category: ExcludeCategory::Cache,
        reason: "uv Python-Paket-Cache",
        exclude_paths: &[".local/share/uv"],
    },
    KnownPattern {
        check_path: ".rye/py",
        category: ExcludeCategory::Toolchain,
        reason: "Rye Python-Versionen",
        exclude_paths: &[".rye/py"],
    },
    KnownPattern {
        check_path: ".pyenv/versions",
        category: ExcludeCategory::Toolchain,
        reason: "pyenv Python-Versionen (pyenv install regeneriert)",
        exclude_paths: &[".pyenv/versions"],
    },
    // ─── Java / JVM ───────────────────────────────────────
    KnownPattern {
        check_path: ".gradle/caches",
        category: ExcludeCategory::Cache,
        reason: "Gradle Build- & Dependency-Cache",
        exclude_paths: &[
            ".gradle/caches",
            ".gradle/daemon",
            ".gradle/wrapper/dists",
            ".gradle/native",
        ],
    },
    KnownPattern {
        check_path: ".m2/repository",
        category: ExcludeCategory::Cache,
        reason: "Maven Local Repository (mvn install regeneriert)",
        exclude_paths: &[".m2/repository"],
    },
    KnownPattern {
        check_path: ".sdkman/candidates",
        category: ExcludeCategory::Toolchain,
        reason: "SDKMAN Java/Kotlin/Gradle-Versionen",
        exclude_paths: &[".sdkman/candidates", ".sdkman/archives"],
    },
    KnownPattern {
        check_path: ".sbt/boot",
        category: ExcludeCategory::Cache,
        reason: "SBT Scala Build-Cache",
        exclude_paths: &[".sbt/boot"],
    },
    KnownPattern {
        check_path: ".coursier/cache",
        category: ExcludeCategory::Cache,
        reason: "Coursier JVM Dependency-Cache",
        exclude_paths: &[".coursier/cache"],
    },
    KnownPattern {
        check_path: ".ivy2/cache",
        category: ExcludeCategory::Cache,
        reason: "Ivy2 Cache (Scala/SBT)",
        exclude_paths: &[".ivy2/cache"],
    },
    // ─── .NET / C# ───────────────────────────────────────
    KnownPattern {
        check_path: ".nuget/packages",
        category: ExcludeCategory::Cache,
        reason: "NuGet-Paket-Cache (dotnet restore regeneriert)",
        exclude_paths: &[".nuget/packages"],
    },
    KnownPattern {
        check_path: ".dotnet/tools",
        category: ExcludeCategory::Toolchain,
        reason: ".NET Global Tools",
        exclude_paths: &[".dotnet/tools"],
    },
    KnownPattern {
        check_path: ".templateengine",
        category: ExcludeCategory::Cache,
        reason: ".NET Template Engine Cache",
        exclude_paths: &[".templateengine"],
    },
    // ─── Go ───────────────────────────────────────────────
    KnownPattern {
        check_path: "go/pkg",
        category: ExcludeCategory::Cache,
        reason: "Go Module Cache (go mod download regeneriert)",
        exclude_paths: &["go/pkg"],
    },
    KnownPattern {
        check_path: "go/bin",
        category: ExcludeCategory::Cache,
        reason: "Go installierte Binaries (go install regeneriert)",
        exclude_paths: &["go/bin"],
    },
    // ─── Ruby ─────────────────────────────────────────────
    KnownPattern {
        check_path: ".gem",
        category: ExcludeCategory::Cache,
        reason: "RubyGems Cache",
        exclude_paths: &[".gem"],
    },
    KnownPattern {
        check_path: ".bundle/cache",
        category: ExcludeCategory::Cache,
        reason: "Bundler Gem-Cache",
        exclude_paths: &[".bundle/cache"],
    },
    KnownPattern {
        check_path: ".rbenv/versions",
        category: ExcludeCategory::Toolchain,
        reason: "rbenv Ruby-Versionen",
        exclude_paths: &[".rbenv/versions"],
    },
    KnownPattern {
        check_path: ".rvm/gems",
        category: ExcludeCategory::Toolchain,
        reason: "RVM Ruby Gems & Versionen",
        exclude_paths: &[".rvm/gems", ".rvm/rubies"],
    },
    // ─── PHP ──────────────────────────────────────────────
    KnownPattern {
        check_path: ".composer/cache",
        category: ExcludeCategory::Cache,
        reason: "Composer PHP-Paket-Cache",
        exclude_paths: &[".composer/cache"],
    },
    // ─── Elixir / Erlang ──────────────────────────────────
    KnownPattern {
        check_path: ".mix/archives",
        category: ExcludeCategory::Cache,
        reason: "Mix Archives (Elixir)",
        exclude_paths: &[".mix/archives"],
    },
    KnownPattern {
        check_path: ".hex/packages",
        category: ExcludeCategory::Cache,
        reason: "Hex-Paket-Cache (Elixir)",
        exclude_paths: &[".hex/packages"],
    },
    KnownPattern {
        check_path: ".asdf/installs",
        category: ExcludeCategory::Toolchain,
        reason: "asdf Tool-Versionen (asdf install regeneriert)",
        exclude_paths: &[".asdf/installs", ".asdf/downloads"],
    },
    KnownPattern {
        check_path: ".local/share/mise/installs",
        category: ExcludeCategory::Toolchain,
        reason: "mise Tool-Versionen",
        exclude_paths: &[".local/share/mise/installs"],
    },
    // ─── Haskell ──────────────────────────────────────────
    KnownPattern {
        check_path: ".stack",
        category: ExcludeCategory::Cache,
        reason: "Haskell Stack Compiler & Dependencies",
        exclude_paths: &[".stack"],
    },
    KnownPattern {
        check_path: ".cabal/store",
        category: ExcludeCategory::Cache,
        reason: "Cabal Package Store",
        exclude_paths: &[".cabal/store"],
    },
    // ─── C/C++ ────────────────────────────────────────────
    KnownPattern {
        check_path: ".ccache",
        category: ExcludeCategory::Cache,
        reason: "ccache Compiler-Cache",
        exclude_paths: &[".ccache"],
    },
    KnownPattern {
        check_path: ".conan/data",
        category: ExcludeCategory::Cache,
        reason: "Conan C++ Package Cache",
        exclude_paths: &[".conan/data", ".conan2/p"],
    },
    KnownPattern {
        check_path: ".vcpkg",
        category: ExcludeCategory::Cache,
        reason: "vcpkg C/C++ Package Manager",
        exclude_paths: &[".vcpkg"],
    },
    // ─── Android / Mobile ─────────────────────────────────
    KnownPattern {
        check_path: "Android/Sdk",
        category: ExcludeCategory::Toolchain,
        reason: "Android SDK (SDK Manager/Android Studio regeneriert)",
        exclude_paths: &["Android/Sdk"],
    },
    KnownPattern {
        check_path: ".android/avd",
        category: ExcludeCategory::VirtualMachine,
        reason: "Android Emulator AVD-Images",
        exclude_paths: &[".android/avd"],
    },
    // ─── IDE & Editor Caches ──────────────────────────────
    KnownPattern {
        check_path: ".local/share/JetBrains",
        category: ExcludeCategory::Runtime,
        reason: "JetBrains IDE Caches & Indizes",
        exclude_paths: &[".local/share/JetBrains"],
    },
    KnownPattern {
        check_path: ".config/JetBrains/*/caches",
        category: ExcludeCategory::Cache,
        reason: "JetBrains IDE Projekt-Caches",
        exclude_paths: &[],
    },
    KnownPattern {
        check_path: ".vscode/extensions",
        category: ExcludeCategory::Runtime,
        reason: "VS Code Extensions (Marketplace regeneriert)",
        exclude_paths: &[".vscode/extensions"],
    },
    KnownPattern {
        check_path: ".vscode-server",
        category: ExcludeCategory::Runtime,
        reason: "VS Code Remote Server (automatisch installiert)",
        exclude_paths: &[".vscode-server"],
    },
    KnownPattern {
        check_path: ".local/share/code-server",
        category: ExcludeCategory::Runtime,
        reason: "code-server Daten",
        exclude_paths: &[".local/share/code-server"],
    },
    KnownPattern {
        check_path: ".eclipse",
        category: ExcludeCategory::Runtime,
        reason: "Eclipse IDE Workspace-Cache",
        exclude_paths: &[".eclipse"],
    },
    KnownPattern {
        check_path: ".local/share/nvim/lazy",
        category: ExcludeCategory::Runtime,
        reason: "Neovim Lazy.nvim Plugins (werden automatisch installiert)",
        exclude_paths: &[".local/share/nvim/lazy", ".local/share/nvim/mason"],
    },
    // ─── Cloud & DevOps Caches ────────────────────────────
    KnownPattern {
        check_path: ".terraform.d/plugin-cache",
        category: ExcludeCategory::Cache,
        reason: "Terraform Provider Cache (terraform init regeneriert)",
        exclude_paths: &[".terraform.d/plugin-cache"],
    },
    KnownPattern {
        check_path: ".local/share/helm/cache",
        category: ExcludeCategory::Cache,
        reason: "Helm Chart Cache",
        exclude_paths: &[".local/share/helm/cache"],
    },
    KnownPattern {
        check_path: ".kube/cache",
        category: ExcludeCategory::Cache,
        reason: "kubectl Discovery Cache",
        exclude_paths: &[".kube/cache"],
    },
    // ═══ Gaming ═══════════════════════════════════════════
    KnownPattern {
        check_path: ".local/share/Steam/steamapps/common",
        category: ExcludeCategory::Gaming,
        reason: "Steam Spieldateien (Steam Download regeneriert)",
        exclude_paths: &[
            ".local/share/Steam/steamapps/common",
            ".local/share/Steam/steamapps/shadercache",
            ".local/share/Steam/steamapps/compatdata",
            ".local/share/Steam/steamapps/downloading",
            ".local/share/Steam/steamapps/temp",
        ],
    },
    KnownPattern {
        check_path: ".local/share/Steam/ubuntu12_32",
        category: ExcludeCategory::Gaming,
        reason: "Steam Runtime (wird automatisch aktualisiert)",
        exclude_paths: &[
            ".local/share/Steam/ubuntu12_32",
            ".local/share/Steam/ubuntu12_64",
        ],
    },
    KnownPattern {
        check_path: ".var/app/com.valvesoftware.Steam/data/Steam/steamapps/common",
        category: ExcludeCategory::Gaming,
        reason: "Flatpak Steam Spieldateien",
        exclude_paths: &[
            ".var/app/com.valvesoftware.Steam/data/Steam/steamapps/common",
            ".var/app/com.valvesoftware.Steam/data/Steam/steamapps/shadercache",
            ".var/app/com.valvesoftware.Steam/data/Steam/steamapps/compatdata",
        ],
    },
    KnownPattern {
        check_path: ".wine",
        category: ExcludeCategory::Gaming,
        reason: "Wine-Prefix (Konfiguration + installierte Programme)",
        exclude_paths: &[".wine"],
    },
    KnownPattern {
        check_path: ".local/share/Steam/compatibilitytools.d",
        category: ExcludeCategory::Gaming,
        reason: "Proton-GE/Custom Proton Versionen",
        exclude_paths: &[".local/share/Steam/compatibilitytools.d"],
    },
    KnownPattern {
        check_path: ".local/share/lutris/runners",
        category: ExcludeCategory::Gaming,
        reason: "Lutris Wine/Proton Runner (Lutris Download regeneriert)",
        exclude_paths: &[".local/share/lutris/runners", ".local/share/lutris/runtime"],
    },
    KnownPattern {
        check_path: ".local/share/heroic",
        category: ExcludeCategory::Gaming,
        reason: "Heroic Games Launcher (GOG/Epic Spieldateien)",
        exclude_paths: &[".local/share/heroic"],
    },
    KnownPattern {
        check_path: ".config/heroic/tools",
        category: ExcludeCategory::Gaming,
        reason: "Heroic Proton/Wine-Versionen",
        exclude_paths: &[".config/heroic/tools"],
    },
    KnownPattern {
        check_path: ".local/share/bottles",
        category: ExcludeCategory::Gaming,
        reason: "Bottles Wine-Container & -Prefixe",
        exclude_paths: &[".local/share/bottles"],
    },
    KnownPattern {
        check_path: ".local/share/itch",
        category: ExcludeCategory::Gaming,
        reason: "itch.io Spieldateien",
        exclude_paths: &[".local/share/itch"],
    },
    KnownPattern {
        check_path: ".local/share/legendary",
        category: ExcludeCategory::Gaming,
        reason: "Legendary (Epic Games CLI) Spieldateien",
        exclude_paths: &[".local/share/legendary"],
    },
    KnownPattern {
        check_path: ".config/legendary/installed.json",
        category: ExcludeCategory::Gaming,
        reason: "Legendary Install-Daten",
        exclude_paths: &[],
    },
    KnownPattern {
        check_path: "Games",
        category: ExcludeCategory::Gaming,
        reason: "Games-Verzeichnis (Symlink oder Spieldaten)",
        exclude_paths: &["Games"],
    },
    // ─── GPU Shader Caches ───────────────────────────────
    KnownPattern {
        check_path: ".local/share/vulkan",
        category: ExcludeCategory::Cache,
        reason: "Vulkan Pipeline Cache",
        exclude_paths: &[".local/share/vulkan"],
    },
    KnownPattern {
        check_path: ".nv",
        category: ExcludeCategory::Cache,
        reason: "NVIDIA Shader & GL Cache",
        exclude_paths: &[".nv"],
    },
    KnownPattern {
        check_path: ".local/share/mesa_shader_cache",
        category: ExcludeCategory::Cache,
        reason: "Mesa/AMD Shader Cache (wird beim Spielen neu generiert)",
        exclude_paths: &[
            ".local/share/mesa_shader_cache",
            ".local/share/mesa_shader_cache_db",
        ],
    },
    KnownPattern {
        check_path: ".AMD",
        category: ExcludeCategory::Cache,
        reason: "AMD Driver Cache",
        exclude_paths: &[".AMD"],
    },
    // ═══ Containers, VMs & Virtualisierung ════════════════
    KnownPattern {
        check_path: ".local/share/docker",
        category: ExcludeCategory::Container,
        reason: "Docker Images, Container & Volumes",
        exclude_paths: &[".local/share/docker"],
    },
    KnownPattern {
        check_path: ".local/share/containers",
        category: ExcludeCategory::Container,
        reason: "Podman/Buildah Container & Images",
        exclude_paths: &[".local/share/containers"],
    },
    KnownPattern {
        check_path: ".local/share/gnome-boxes",
        category: ExcludeCategory::VirtualMachine,
        reason: "GNOME Boxes VM-Disk-Images",
        exclude_paths: &[".local/share/gnome-boxes"],
    },
    KnownPattern {
        check_path: ".local/share/libvirt",
        category: ExcludeCategory::VirtualMachine,
        reason: "libvirt/QEMU VM-Images",
        exclude_paths: &[".local/share/libvirt"],
    },
    KnownPattern {
        check_path: "VirtualBox VMs",
        category: ExcludeCategory::VirtualMachine,
        reason: "VirtualBox VM-Disk-Images",
        exclude_paths: &["VirtualBox VMs"],
    },
    KnownPattern {
        check_path: ".vagrant.d/boxes",
        category: ExcludeCategory::VirtualMachine,
        reason: "Vagrant Box-Images",
        exclude_paths: &[".vagrant.d/boxes"],
    },
    // ─── Flatpak & Snap Caches ───────────────────────────
    KnownPattern {
        check_path: ".local/share/flatpak",
        category: ExcludeCategory::Runtime,
        reason: "Flatpak lokale Installationen (flatpak install regeneriert)",
        exclude_paths: &[".local/share/flatpak"],
    },
    KnownPattern {
        check_path: "snap",
        category: ExcludeCategory::Runtime,
        reason: "Snap Anwendungsdaten",
        exclude_paths: &["snap"],
    },
    // ─── Media & Kreativ-Tools ───────────────────────────
    KnownPattern {
        check_path: ".local/share/shotwell",
        category: ExcludeCategory::Media,
        reason: "Shotwell Foto-Thumbnails",
        exclude_paths: &[".local/share/shotwell"],
    },
    KnownPattern {
        check_path: ".gimp-*/tmp",
        category: ExcludeCategory::Media,
        reason: "GIMP Temp-Dateien",
        exclude_paths: &[],
    },
    KnownPattern {
        check_path: ".local/share/gvfs-metadata",
        category: ExcludeCategory::Cache,
        reason: "GVFS Metadaten-Cache",
        exclude_paths: &[".local/share/gvfs-metadata"],
    },
    // ─── Logs & Crash Reports ────────────────────────────
    KnownPattern {
        check_path: ".xsession-errors",
        category: ExcludeCategory::Cache,
        reason: "X11 Session-Fehlerlog",
        exclude_paths: &[".xsession-errors", ".xsession-errors.old"],
    },
    KnownPattern {
        check_path: ".local/share/xorg",
        category: ExcludeCategory::Cache,
        reason: "Xorg Logs",
        exclude_paths: &[".local/share/xorg"],
    },
];
