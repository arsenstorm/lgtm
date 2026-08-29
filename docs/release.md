# Releases

## Cutting one

1. Bump `version` under `[workspace.package]` in `Cargo.toml`.
2. `cargo build --workspace` so `Cargo.lock` picks up the new version.
3. `git commit -am "chore: release v0.2.0"`
4. `git tag v0.2.0 && git push origin main v0.2.0`

The tag push runs `.github/workflows/release.yml`, which:

- fails immediately if the tag does not match the `Cargo.toml` version;
- builds `lgtm` for `aarch64-apple-darwin`, `x86_64-apple-darwin`,
  `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
  `armv7-unknown-linux-gnueabihf` (the Linux ARM pair via
  [`cross`](https://github.com/cross-rs/cross)), and
  `x86_64-pc-windows-msvc`;
- publishes a GitHub release with one archive per target, a `SHA256SUMS`
  file, `install.sh`, `install.ps1`, and generated release notes.

A tag containing `-` (`v0.2.0-rc.1`) is published as a pre-release, so
`/releases/latest/download/...` keeps pointing at the last stable build.

Everything is a plain Rust build — TLS is `ring`-only, so no target needs
cmake or a C toolchain beyond what the runner already has.

## Installing

```sh
curl -fsSL https://lgtm.arsenstorm.com/install | bash
```

```powershell
powershell -c "irm lgtm.arsenstorm.com/install.ps1 | iex"
```

Both drop the binary in `~/.lgtm/bin` and add that directory to `PATH`.
`LGTM_VERSION=v0.2.0` installs a specific tag; `LGTM_RELEASE_BASE` points the
script at a different releases host (used by the local test in this repo).

## Upgrading

Re-run the install line. It overwrites `~/.lgtm/bin/lgtm` and is a no-op on

## Updating

Re-run the install line, or run `lgtm upgrade` (`--version vX.Y.Z` for a specific release). It downloads the asset for the running platform, verifies it against `SHA256SUMS`, and replaces the binary in place.

## Cloudflare

Two redirect rules on `lgtm.arsenstorm.com` (Rules → Redirect Rules, status
302) keep the install URLs short:

| Path | Destination |
| --- | --- |
| `/install` | `https://github.com/arsenstorm/lgtm/releases/latest/download/install.sh` |
| `/install.ps1` | `https://github.com/arsenstorm/lgtm/releases/latest/download/install.ps1` |

Both sides follow redirects: `curl -fsSL` because of `-L`, and PowerShell's
`irm` (`Invoke-RestMethod`) by default. Use 302 rather than 301 so the
destination can move without cached redirects following the old one.

## macOS Gatekeeper

The CLI is unsigned, but Gatekeeper only quarantines files downloaded by an
app that sets the quarantine attribute (Safari, Chrome). `curl` and the
install script do not, so no `xattr -d com.apple.quarantine` step is needed.
A user who downloads the tarball by hand from the releases page in a browser
does need it:

```sh
xattr -d com.apple.quarantine ~/.lgtm/bin/lgtm
```

The desktop app will need real signing and notarisation when it ships.
