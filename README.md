# AdobeCraft

A single repository holding pinned copies of the seven [storytold](https://github.com/storytold) "craft" projects.

The goal is stability: each project is frozen at a known-good commit and only changes when it is deliberately updated. Upstream changes never arrive on their own.

**Snapshot taken: 2026-10-09**

## Pinned versions

| Folder | Replaces | Upstream | Commit | Commit date |
|---|---|---|---|---|
| `photocraft/` | Photoshop | [storytold/photocraft](https://github.com/storytold/photocraft) | `2515fa7ce` | 2026-10-09 |
| `vectorcraft/` | Illustrator | [storytold/vectorcraft](https://github.com/storytold/vectorcraft) | `46e426237` | 2026-10-09 |
| `filmcraft/` | Premiere | [storytold/filmcraft](https://github.com/storytold/filmcraft) | `523185244` (v0.4.0) | 2026-10-08 |
| `lightcraft/` | Lightroom | [storytold/lightcraft](https://github.com/storytold/lightcraft) | `c435d143d` | 2026-10-09 |
| `printcraft/` | Acrobat Pro | [storytold/printcraft](https://github.com/storytold/printcraft) | `a6bc63a49` | 2026-10-08 |
| `effectcraft/` | After Effects | [storytold/effectcraft](https://github.com/storytold/effectcraft) | `cae67546d` | 2026-10-09 |
| `designcraft/` | InDesign | [storytold/designcraft](https://github.com/storytold/designcraft) | `14e677b24` (v0.4.0) | 2026-10-08 |

All are taken from each upstream's `main` branch.

## How it works

Each folder is a [git subtree](https://git-scm.com/book/en/v2/Git-Tools-Advanced-Merging#_subtree_merge): a full copy of the upstream code and history, stored in this repo. If an upstream repository changes or disappears, the copy here is unaffected.

## Updating

Run from Git Bash (or any POSIX shell) in the repo root:

```sh
sh update.sh                # update all seven projects
sh update-photocraft.sh     # update one project (likewise for the others)
```

Each update pulls the upstream `main` and records it as a merge commit. Review it with `git show`; undo it with `git reset --hard HEAD~1`.

After updating, bump the snapshot date and the table above to match.
