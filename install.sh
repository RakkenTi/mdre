#!/bin/sh
# mdre installer — Linux and macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/RakkenTi/mdre/main/install.sh | sh
#
# Downloads the release binary for this machine, checks it against the
# release's SHA256SUMS, and installs it without ever asking for root.
#
# Environment:
#   MDRE_VERSION       tag to install (default: the latest release)
#   MDRE_INSTALL_DIR   where to put the binary (default: ~/.local/bin)
#
# Windows is not covered here; download the .zip from the releases page.

set -eu

REPO="RakkenTi/mdre"
INSTALL_DIR="${MDRE_INSTALL_DIR:-$HOME/.local/bin}"

RED=''; GREEN=''; YELLOW=''; BOLD=''; OFF=''
if [ -t 2 ] && [ -z "${NO_COLOR:-}" ]; then
    RED=$(printf '\033[31m'); GREEN=$(printf '\033[32m')
    YELLOW=$(printf '\033[33m'); BOLD=$(printf '\033[1m'); OFF=$(printf '\033[0m')
fi

say()  { printf '%s\n' "$*" >&2; }
warn() { printf '%swarning%s %s\n' "$YELLOW" "$OFF" "$*" >&2; }
die()  { printf '%serror%s %s\n' "$RED" "$OFF" "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || die "$1 is required but not installed"
}

# --------------------------------------------------------------- downloading

# Failures are reported by the caller with more context than curl or wget
# gives, so keep their own noise out of the way.
if command -v curl >/dev/null 2>&1; then
    fetch()  { curl -fsSL "$1" -o "$2" 2>/dev/null; }
    fetch_stdout() { curl -fsSL "$1" 2>/dev/null; }
elif command -v wget >/dev/null 2>&1; then
    fetch()  { wget -qO "$2" "$1" 2>/dev/null; }
    fetch_stdout() { wget -qO- "$1" 2>/dev/null; }
else
    die "neither curl nor wget is installed"
fi

# ------------------------------------------------------------ this machine

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)

    case "$os" in
        Linux)  os_part="unknown-linux-musl" ;;
        Darwin) os_part="apple-darwin" ;;
        MINGW*|MSYS*|CYGWIN*)
            die "Windows is not supported by this script.
  Download the .zip from https://github.com/$REPO/releases and add it to your PATH." ;;
        *) die "unsupported operating system: $os" ;;
    esac

    case "$arch" in
        x86_64|amd64)  arch_part="x86_64" ;;
        aarch64|arm64) arch_part="aarch64" ;;
        *) die "unsupported architecture: $arch
  Prebuilt binaries exist for x86_64 and aarch64 only.
  You can build from source instead: cargo install mdre" ;;
    esac

    printf '%s-%s' "$arch_part" "$os_part"
}

latest_version() {
    # Read the tag straight out of the redirect to /releases/latest rather than
    # the API, which rate-limits unauthenticated callers hard enough to matter.
    tag=$(fetch_stdout "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
        | sed -n 's/.*"tag_name" *: *"\([^"]*\)".*/\1/p' | head -1) || true
    [ -n "${tag:-}" ] || die "could not determine the latest version.
  Set one explicitly:  MDRE_VERSION=v0.1.0 sh install.sh")
    printf '%s' "$tag"
}

# ------------------------------------------------------------------ install

main() {
    need uname; need tar; need mkdir

    target=$(detect_target)
    version="${MDRE_VERSION:-$(latest_version)}"
    case "$version" in v*) ;; *) version="v$version" ;; esac

    archive="mdre-$version-$target.tar.gz"
    base="https://github.com/$REPO/releases/download/$version"

    say "${BOLD}mdre${OFF} $version  ($target)"

    tmp=$(mktemp -d 2>/dev/null || mktemp -d -t mdre)
    # Leave nothing behind, including when the download fails halfway.
    trap 'rm -rf "$tmp"' EXIT INT TERM

    say "  downloading $archive"
    fetch "$base/$archive" "$tmp/$archive" \
        || die "could not download $archive
  Either that asset does not exist or the network is unavailable.
  Published releases: https://github.com/$REPO/releases"

    # A pipe from the internet into a shell is only as trustworthy as what it
    # runs, so verify the payload before unpacking it.
    if fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" 2>/dev/null; then
        expected=$(sed -n "s/^\([0-9a-f]\{64\}\)  *$archive\$/\1/p" "$tmp/SHA256SUMS" | head -1)
        if [ -z "$expected" ]; then
            warn "SHA256SUMS has no entry for $archive — skipping verification"
        else
            if command -v sha256sum >/dev/null 2>&1; then
                actual=$(sha256sum "$tmp/$archive" | cut -d' ' -f1)
            elif command -v shasum >/dev/null 2>&1; then
                actual=$(shasum -a 256 "$tmp/$archive" | cut -d' ' -f1)
            else
                actual=""
                warn "no sha256sum or shasum — skipping verification"
            fi
            if [ -n "$actual" ]; then
                [ "$actual" = "$expected" ] || die "checksum mismatch for $archive
  expected $expected
  got      $actual
  Do not use this download."
                say "  ${GREEN}checksum ok${OFF}"
            fi
        fi
    else
        warn "no SHA256SUMS in this release — skipping verification"
    fi

    tar xzf "$tmp/$archive" -C "$tmp"
    binary="$tmp/mdre-$version-$target/mdre"
    [ -f "$binary" ] || binary=$(find "$tmp" -name mdre -type f | head -1)
    [ -n "$binary" ] && [ -f "$binary" ] || die "the archive did not contain an mdre binary"

    mkdir -p "$INSTALL_DIR" || die "cannot create $INSTALL_DIR"
    # Install to a temporary name and rename, so a running mdre is never
    # replaced underneath itself half-written.
    chmod 755 "$binary"
    cp "$binary" "$INSTALL_DIR/.mdre.new" || die "cannot write to $INSTALL_DIR
  Pick somewhere else:  MDRE_INSTALL_DIR=~/bin sh install.sh"
    mv "$INSTALL_DIR/.mdre.new" "$INSTALL_DIR/mdre"

    say "  ${GREEN}installed${OFF} $INSTALL_DIR/mdre"

    # An installed binary the shell cannot find is the most common way this
    # goes wrong, so say it plainly instead of leaving them to work it out.
    case ":$PATH:" in
        *":$INSTALL_DIR:"*)
            say ""
            say "Run ${BOLD}mdre${OFF} to get started, or ${BOLD}mdre --help${OFF}."
            ;;
        *)
            say ""
            warn "$INSTALL_DIR is not on your PATH."
            say "  Add it by appending this to your shell profile:"
            say ""
            say "    export PATH=\"\$PATH:$INSTALL_DIR\""
            say ""
            say "  Then restart your shell, or run it now to use mdre immediately."
            ;;
    esac
}

main "$@"
