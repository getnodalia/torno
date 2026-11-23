# Skills for working on Torno

Skills for contributors and their coding assistants: claiming work, writing and watching a pull request, and the rules for each area of the code. Each folder is one skill in the [Agent Skills](https://agentskills.io) format. `AGENTS.md` routes to them.

Coding assistants that read project skills from `.agents/skills/` find them here. `.claude/skills` is a symlink to this folder. On Windows, enable symlinks (Developer Mode, then `git config core.symlinks true`) or point the tool at this folder.

These are not built into the app. The skills for agents that model through Torno's MCP server are in [`skills/`](../../skills/README.md).

| Skill | Loads when the agent |
|---|---|
| [`torno-contributing`](torno-contributing/SKILL.md) | helps someone set up a fork, choose and claim an issue, and submit a contribution for review; the rules every PR follows |
| [`torno-file-pr`](torno-file-pr/SKILL.md) | opens or updates a pull request: the title, the body, the template lines the checks read, screenshots for UI changes |
| [`torno-babysit-pr`](torno-babysit-pr/SKILL.md) | has pushed a pull request: the local check, waiting for CI and the review bot, reading results, fixing real findings |
| [`torno-full-stop`](torno-full-stop/SKILL.md) | is asked to take a change all the way to a pull request ready for review, with nobody watching; never merges |
| [`torno-takeover`](torno-takeover/SKILL.md) | finishes someone else's pull request or stalled claim and keeps their credit |
| [`torno-modernize`](torno-modernize/SKILL.md) | brings an old pull request up to date and decides whether to keep, rework or close it |
| [`torno-ground-rules`](torno-ground-rules/SKILL.md) | adds a dependency, asset or wording: independence, licences, attribution, the product name |
| [`torno-crash-safety`](torno-crash-safety/SKILL.md) | writes non-test Rust, handles hostile input or investigates a crash report |
| [`torno-commands`](torno-commands/SKILL.md) | adds a command or decides which crate code belongs in |
| [`torno-layers`](torno-layers/SKILL.md) | decides whether code goes in Torno or solidcore (kernel, ops), or works on kernel or geometry code |
| [`torno-ui`](torno-ui/SKILL.md) | adds a dialog, panel, camera input, icon or colour |
| [`torno-shared-controls`](torno-shared-controls/SKILL.md) | changes selection, picking, keys or a tool's start and finish, or wants a new option: shared controls, and options in the tool's panel |
| [`torno-ui-text`](torno-ui-text/SKILL.md) | adds a label, message or translation |
| [`torno-file-formats`](torno-file-formats/SKILL.md) | changes what a design saves, or touches import and export |
| [`torno-preferences`](torno-preferences/SKILL.md) | adds a setting or changes a default |
| [`torno-kernel-rules`](torno-kernel-rules/SKILL.md) | changes geometry, solidcore or a test expectation: one implementation, no fix flags, no new refusals of valid input, test changes with a derivation |
| [`torno-repro-first`](torno-repro-first/SKILL.md) | fixes a bug seen only on some machines (an OS, graphics driver, display scaling, the updater): evidence from a released build, a fix scoped to that condition |
| [`torno-tests`](torno-tests/SKILL.md) | places or runs tests |
| [`torno-performance`](torno-performance/SKILL.md) | works on slow code, the window thread or the wasm build |
| [`torno-packaging`](torno-packaging/SKILL.md) | prepares a release or changes packages |
| [`torno-agent-tools`](torno-agent-tools/SKILL.md) | runs the app, drives the control channel or changes MCP tools |

`torno-file-pr`, `torno-babysit-pr` (with its `pr-wait` watcher), `torno-full-stop`, `torno-takeover` and `torno-modernize` are adapted from [tstack](https://github.com/t3dotgg/tstack) by Theo Browne, MIT licence ([LICENSE-tstack](LICENSE-tstack)).
