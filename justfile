name := 'cosmic-applet-trash'
appid := 'com.github.abde.cosmic-applet-trash'
rootdir := ''
prefix := '/usr'

appdata := appid + '.metainfo.xml'
desktop := appid + '.desktop'

# Installation paths
base-dir := absolute_path(clean(rootdir / prefix))
cargo-target-dir := env('CARGO_TARGET_DIR', 'target')
appdata-dst := base-dir / 'share' / 'appdata' / appdata
bin-dst := base-dir / 'bin' / name
desktop-dst := base-dir / 'share' / 'applications' / desktop
icon-dst := base-dir / 'share' / 'icons' / 'hicolor' / 'scalable' / 'apps' / appid + '.svg'

# Default recipe which runs `just build-release`
default: build-release

# Runs `cargo clean`
clean:
    cargo clean

# Compiles with debug profile
build-debug *args:
    cargo build {{args}}

# Compiles with release profile
build-release *args: (build-debug '--release' args)

# Runs a clippy check
check *args:
    cargo clippy --all-features {{args}}

# Run the application for testing purposes
run *args:
    env RUST_BACKTRACE=full cargo run --release {{args}}

# Installs files to system /usr
install:
    install -Dm0755 {{ cargo-target-dir / 'release' / name }} {{bin-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.desktop' }} {{desktop-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.metainfo.xml' }} {{appdata-dst}}
    install -Dm0644 resources/icon.svg {{icon-dst}}

# Installs files to ~/.local for local user without sudo
user-install:
    mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons/hicolor/scalable/apps
    install -Dm0755 {{ cargo-target-dir / 'release' / name }} ~/.local/bin/{{name}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.desktop' }} ~/.local/share/applications/{{desktop}}
    install -Dm0644 resources/icon.svg ~/.local/share/icons/hicolor/scalable/apps/{{appid}}.svg

# Uninstalls installed files
uninstall:
    rm -f {{bin-dst}} {{desktop-dst}} {{icon-dst}} ~/.local/bin/{{name}} ~/.local/share/applications/{{desktop}}
