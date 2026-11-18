Torno is open-source parametric 3D CAD in Rust: sketch, constrain, extrude, and change a
parameter to watch the timeline rebuild. [Documentation](https://getnodalia.github.io/torno/)
· [try it in the browser](https://getnodalia.github.io/torno/app) ·
[contributing](https://github.com/bherbruck/solidcraft/blob/main/CONTRIBUTING.md) ·
[issues](https://github.com/getnodalia/torno/issues). AI agents can model with it over MCP:
`torno-cli mcp` ([how](https://getnodalia.github.io/torno/guide/mcp.html)).

**What's new** in Torno {version} is listed below, and the
[roadmap detail](https://github.com/bherbruck/solidcraft/blob/main/docs/roadmap-detail.md#known-limitations) lists
the known limitations. Designs from every earlier version open in this one.

**New app ID.** The app ID is now `dev.danielmiguelt.Torno` (it was `ai.storyteller.solidcraft`): `flatpak update` offers to replace the old app, the
`.deb`, `.rpm` and AppImage remove the old menu entry, and settings stay. On macOS the bundle ID
changed too, so macOS asks once more for the permissions it had granted.

**Old names.** The zip container is the default save format. Want plain JSON? Use
**File ▸ Save As JSON…** or `--save design.json`. Designs saved by earlier versions, including
`.solvecraft` files, still open; the `solvecraft-cli` command, `SOLVECRAFT_*` variables and the
`.solvecraft` file association are gone. Details:
[compatibility notes](https://github.com/bherbruck/solidcraft/blob/main/docs/compatibility.md).

Documentation, the browser app and the Flatpak feed use the Torno project Pages URLs.

**Which file?**
- **Windows:** `Torno-{version}-windows-x64.exe` runs as is. Or install the `.msi`, or unzip the
  `-portable.zip` (x64, x86 and arm64). IT admins: `torno-{version}-windows-policy-templates.zip`
  holds the Intune and Group Policy templates ([Managing Torno in an organization](https://github.com/tangency-labs/solidcraft/blob/main/docs/book/src/guide/managed-deployment.md)).
- **macOS 11+** (Apple silicon and Intel): open the `.dmg` and drag `Torno.app` onto Applications.
  The CLI is in `torno-cli-{version}-macos-universal.zip`.
- **Linux** (x86_64 and aarch64, glibc 2.35+): the `.AppImage` (`chmod +x`, then run it), the
  `.deb` or `.rpm`, the `.flatpak` (`flatpak install --user <file>`), the `.snap`
  (`sudo snap install --dangerous <file>`, or `sudo snap install torno` from the Snap Store), or the `.tar.gz`.
- **FreeBSD:** `tar -xzf torno-{version}-freebsd-x86_64.tar.gz --strip-components 1 -C /usr/local`.
- **Web:** `torno-web-{version}.zip` is a static site to host yourself.
- `SHA256SUMS.txt`: `sha256sum -c SHA256SUMS.txt --ignore-missing`.
- Files without a version in the name (`torno-windows-x64.msi`, `torno-linux-x86_64.AppImage`, …)
  are the same packages under fixed names, for download links that always fetch the latest release.

For Flatpak feed installation and updates, add Flathub for the required runtime before
installing Torno; keep both remotes in the same user installation:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak remote-add --user --if-not-exists torno https://getnodalia.github.io/torno/flatpak/torno.flatpakrepo
flatpak install --user torno dev.danielmiguelt.Torno
flatpak run dev.danielmiguelt.Torno
```

A remote added from the old `bherbruck.github.io` address no longer finds updates: run
`flatpak remote-modify --user --url=https://getnodalia.github.io/torno/flatpak/repo/ torno`, then
`flatpak update --user`.

Flatpak installs under the old ID `ai.storyteller.solidcraft`: `flatpak update --user` offers to replace it with `dev.danielmiguelt.Torno`, and settings move along.

Single-file `.flatpak` bundles include the runtime repository address; missing runtimes
still need an internet connection. [Linux installation details](https://getnodalia.github.io/torno/guide/getting-started.html#linux-flatpak).

The builds aren't signed yet. On macOS, right-click `Torno.app` in Applications and choose
**Open** the first time (or run `xattr -dr com.apple.quarantine /Applications/Torno.app`).
On Windows, SmartScreen may say "Windows protected your PC": **More info**, then **Run anyway**.
Licences are built in (Help ▸ About ▸ Licences, `torno-cli licences`); the packages also
install `THIRD-PARTY-LICENSES.txt`.

