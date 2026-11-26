Torno is open-source parametric 3D CAD in Rust: sketch, constrain, extrude, and change a
parameter to watch the timeline rebuild. [Documentation](https://getnodalia.github.io/torno/)
· [try it in the browser](https://getnodalia.github.io/torno/app) ·
[contributing](https://github.com/getnodalia/torno/blob/main/CONTRIBUTING.md) ·
[issues](https://github.com/getnodalia/torno/issues). AI agents can model with it over MCP:
`torno-cli mcp` ([how](https://getnodalia.github.io/torno/guide/mcp.html)).

**What's new** in Torno {version} is listed below, and the
[roadmap detail](https://github.com/getnodalia/torno/blob/main/docs/roadmap-detail.md#known-limitations) lists
the known limitations. Designs from every earlier version open in this one.

**New app ID.** The app ID is now `dev.danielmiguelt.Torno` (it was `ai.storyteller.solidcraft`): the
`.deb`, `.rpm` and AppImage remove the old menu entry, and settings stay. On macOS the bundle ID
changed too, so macOS asks once more for the permissions it had granted.

**Upgrading from 0.5 or earlier.** 0.6 renamed SolveCraft and made the zip
container the default save format. `.solvecraft` designs, old settings folders and
`SOLVECRAFT_*` variables are still picked up, and `solvecraft-cli` still forwards to
`torno-cli`. Want plain JSON? Use **File ▸ Save As JSON…** or `--save design.json`. On
macOS, replace `SolveCraft.app` with `Torno.app`. Details:
[compatibility notes](https://github.com/getnodalia/torno/blob/main/docs/compatibility.md).

Documentation and the browser app use the Torno project Pages URLs.

**Which file?**
- **Windows:** `Torno-{version}-windows-x64.exe` runs as is. Or install the `.msi`, or unzip the
  `-portable.zip` (x64, x86 and arm64). IT admins: `torno-{version}-windows-policy-templates.zip`
  holds the Intune and Group Policy templates ([Managing Torno in an organization](https://github.com/getnodalia/torno/blob/main/docs/book/src/guide/managed-deployment.md)).
- **macOS 11+** (Apple silicon and Intel): open the `.dmg` and drag `Torno.app` onto Applications.
  The CLI is in `torno-cli-{version}-macos-universal.zip`.
- **Linux** (x86_64 and aarch64, glibc 2.35+): the `.AppImage` (`chmod +x`, then run it), the
  `.deb` or `.rpm`, the `.snap`
  (`sudo snap install --dangerous <file>`, or `sudo snap install torno` from the Snap Store), or the `.tar.gz`.
- **FreeBSD:** `tar -xzf torno-{version}-freebsd-x86_64.tar.gz --strip-components 1 -C /usr/local`.
- **Web:** `torno-web-{version}.zip` is a static site to host yourself.
- `SHA256SUMS.txt`: `sha256sum -c SHA256SUMS.txt --ignore-missing`.
- Files without a version in the name (`torno-windows-x64.msi`, `torno-linux-x86_64.AppImage`, …)
  are the same packages under fixed names, for download links that always fetch the latest release.
