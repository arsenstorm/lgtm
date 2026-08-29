#!/bin/sh
# Install the lgtm CLI into ~/.lgtm/bin.
#
#   curl -fsSL https://lgtm.arsenstorm.com/install | sh
#
# LGTM_VERSION=v0.2.0 pins a tag; LGTM_RELEASE_BASE points at another host.
set -eu

BASE="${LGTM_RELEASE_BASE:-https://github.com/arsenstorm/lgtm/releases}"
VERSION="${LGTM_VERSION:-latest}"
PREFIX="$HOME/.lgtm/bin"

os="$(uname -s)"
arch="$(uname -m)"
case "$os $arch" in
"Darwin arm64") target="aarch64-apple-darwin" ;;
"Darwin x86_64") target="x86_64-apple-darwin" ;;
"Linux x86_64") target="x86_64-unknown-linux-gnu" ;;
"Linux aarch64" | "Linux arm64") target="aarch64-unknown-linux-gnu" ;;
"Linux armv7l" | "Linux armv6l") target="armv7-unknown-linux-gnueabihf" ;;
*)
	echo "unsupported platform: $os $arch" >&2
	exit 1
	;;
esac

if [ "$VERSION" = "latest" ]; then
	url="$BASE/latest/download"
else
	url="$BASE/download/$VERSION"
fi
file="lgtm-$target.tar.gz"

fetch() {
	if command -v curl >/dev/null 2>&1; then
		curl -fsSL "$1" -o "$2"
	elif command -v wget >/dev/null 2>&1; then
		wget -q -O "$2" "$1"
	else
		echo "need curl or wget to download $1" >&2
		exit 1
	fi
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

fetch "$url/$file" "$tmp/$file"
fetch "$url/SHA256SUMS" "$tmp/SHA256SUMS"

cd "$tmp"
# One line, so plain `-c` works on both GNU coreutils and BSD/macOS shasum.
if ! grep " $file\$" SHA256SUMS >expected; then
	echo "no checksum for $file in SHA256SUMS" >&2
	exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
	sha256sum -c expected >/dev/null
elif command -v shasum >/dev/null 2>&1; then
	shasum -a 256 -c expected >/dev/null
else
	echo "need sha256sum or shasum to verify the download" >&2
	exit 1
fi

tar -xzf "$file"
mkdir -p "$PREFIX"
install -m 755 lgtm "$PREFIX/lgtm"

path_added=0

append_rc() {
	rc="$1"
	if [ -f "$rc" ] && grep -q '.lgtm/bin' "$rc"; then
		return 0
	fi
	mkdir -p "$(dirname "$rc")"
	{
		echo ""
		echo "# lgtm"
		echo "$2"
	} >>"$rc"
	path_added=1
}

# shellcheck disable=SC2016 # $HOME stays literal; the rc file expands it.
path_line='export PATH="$HOME/.lgtm/bin:$PATH"'
# shellcheck disable=SC2016
fish_line='fish_add_path "$HOME/.lgtm/bin"'

case ":$PATH:" in
*":$PREFIX:"*) ;;
*)
	case "$(basename "${SHELL:-/bin/sh}")" in
	zsh) append_rc "$HOME/.zshrc" "$path_line" ;;
	bash)
		append_rc "$HOME/.bashrc" "$path_line"
		if [ "$os" = "Darwin" ]; then
			append_rc "$HOME/.bash_profile" "$path_line"
		fi
		;;
	fish) append_rc "$HOME/.config/fish/conf.d/lgtm.fish" "$fish_line" ;;
	*) append_rc "$HOME/.profile" "$path_line" ;;
	esac
	;;
esac

# Prints "lgtm <version>" once the CLI takes --version; plain "lgtm" until then.
version="$("$PREFIX/lgtm" --version 2>/dev/null || echo "lgtm")"
echo "installed $version to ~/.lgtm/bin/lgtm"
if [ "$path_added" = 1 ]; then
	echo "open a new shell or run: $path_line"
fi
echo "next: lgtm serve"
