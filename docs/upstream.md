# Torno upstream

| Field | Value |
|---|---|
| Upstream | `tangency-labs/solidcraft` |
| Last observed | `84418862b0c0f5432f83216fd26f6dda96d87dd5` |
| Last evaluated | `84418862b0c0f5432f83216fd26f6dda96d87dd5` |
| Last integrated | `84418862b0c0f5432f83216fd26f6dda96d87dd5` (6249 commits since the last sync) on 2026-11-18 |
| Skipped | none in the baseline import |
| Adapted | app icon: `assets/icons/app/shared/source/app_icon.rs` resamples a portrait instead of drawing upstream's mark; every SolidCraft name renamed to Torno (`.github/rebrand.toml`) |
| Conflicting | none recorded |

Torno shares history with upstream, but never merges `upstream/main` directly: upstream still
calls the app SolidCraft, and its crates, paths, app id, environment variables and docs say so.
Upstream's name and logo are covered by `LICENSE-BRAND`; Torno uses its own name and icon.

## How upstream arrives renamed

`scripts/rebrand.py` renames upstream's names to Torno's, in file contents and in paths, from the
rules in `.github/rebrand.toml` (literal replacements in order; spans to keep such as links to
upstream's issues, the `.solidcraft` design extension and file format tags; files to skip such as
`NOTICE` and the licences; and lines that keep Torno's own value such as the version). A line
marked `rebrand: keep` is left alone, for compatibility code that has to name the old product
(docs/compatibility.md).

Each sync records upstream's head as one mirror commit: it holds the rebranded tree of that
upstream commit, its only parent is the previous mirror commit, its subject is
`Upstream <sha> with this app's names`, and an `Upstream-Commit:` trailer names the upstream commit
it renames. The weekly sync (`.github/workflows/upstream-sync.yml`) merges that commit into a branch
and opens a pull request, so upstream's changes arrive already speaking Torno, and a conflict is a
real one. Files in `.github/upstream-keep` keep Torno's version. No mirror branch is pushed, so merge
sync pull requests with a merge commit: a squash would drop the mirror commit the next sync starts
from.

The first mirror commit is upstream `87400c80` renamed; it was merged with `-s ours` when the
whole repository was renamed, so only later upstream changes come through it.

`python3 scripts/rebrand.py check` lists anything still using an upstream name.
