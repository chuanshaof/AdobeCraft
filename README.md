# AdobeCraft

A single repository holding pinned copies of the seven [storytold](https://github.com/storytold) "craft" projects.

The goal is stability: each project is frozen at a known-good commit and only changes when it is deliberately updated. Upstream changes never arrive on their own.

**Snapshot taken: 2026-10-24**

## Pinned versions

| Folder | Replaces | Upstream | Commit | Commit date |
|---|---|---|---|---|
| `photocraft/` | Photoshop | [storytold/photocraft](https://github.com/storytold/photocraft) | `1c3db2e5c` | 2026-10-23 |
| `vectorcraft/` | Illustrator | [storytold/vectorcraft](https://github.com/storytold/vectorcraft) | `50d976e1e` | 2026-10-24 |
| `filmcraft/` | Premiere | [storytold/filmcraft](https://github.com/storytold/filmcraft) | `fa550ce2d` | 2026-10-23 |
| `lightcraft/` | Lightroom | [storytold/lightcraft](https://github.com/storytold/lightcraft) | `4ae308942` | 2026-10-23 |
| `printcraft/` | Acrobat Pro | [storytold/printcraft](https://github.com/storytold/printcraft) | `86730ed14` | 2026-10-24 |
| `effectcraft/` | After Effects | [storytold/effectcraft](https://github.com/storytold/effectcraft) | `eb5108786` | 2026-10-23 |
| `designcraft/` | InDesign | [storytold/designcraft](https://github.com/storytold/designcraft) | `5449bc475` | 2026-10-24 |

All are taken from each upstream's `main` branch.

## How it works

Each folder is a [git subtree](https://git-scm.com/book/en/v2/Git-Tools-Advanced-Merging#_subtree_merge): a full copy of the upstream code and history, stored in this repo. If an upstream repository changes or disappears, the copy here is unaffected.

## Updating

Run from Git Bash (or any POSIX shell) in the repo root:

```sh
sh update.sh                              # update all seven projects
sh update-scripts/update-photocraft.sh   # update one project (likewise for the others)
```

Each update pulls the upstream `main` as a merge commit, then rewrites the table above (and the snapshot date, if anything changed) in a follow-up commit. Review with `git log -p`; the script prints the exact `git reset --hard <commit>` to undo it.
