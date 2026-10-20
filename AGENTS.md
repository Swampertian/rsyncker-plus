# Agent Guidelines

## Isolation and collaboration

- Use one Git worktree and one branch per task. Create them with
  `script/workflow/worktree.sh create <agent> <task>`; branches use
  `feat/<task>` and worktrees live under `.worktrees/<agent>-<task>/`.
- Agents work only on their task branches. After committing the finished task
  and passing checks, the coordinator integrates it with
  `script/workflow/worktree.sh integrate <agent> <task>` from the integration checkout.
  This runs `script/check.sh`, merges the branch, and removes its worktree and
  branch on success. Git rejects overlapping local edits; unrelated edits in
  the integration checkout are preserved. Do not merge or modify the
  integration branch directly.
- Edit only files assigned to your task. Re-read shared files and files with
  active edits before making small, localized changes.
- If another agent's in-progress change breaks the build, wait and retry; do not
  fix files you do not own.
- Never share one worktree between agents. Use a worktree-specific Cargo target
  directory to avoid contention on Cargo's build lock:

  ```sh
  CARGO_TARGET_DIR=target/agent-<name> cargo check --all-targets --locked
  CARGO_TARGET_DIR=target/agent-<name> cargo test --all-targets --locked
  ```

- Each `target/agent-*` directory can use about 10 GB. Remove your worktree with
  `script/workflow/worktree.sh remove <agent> <task>` when finished; this also removes
  its local build artifacts. Never remove a target directory belonging to an
  active agent.
- Commit completed work only to the task branch when requested by the
  coordinator; never commit directly to the integration branch. On completion,
  report the branch, changed files, checks run, and any conflicts.

## Rust and Cargo

- Rust 1.85.0 is pinned in `rust-toolchain.toml`. Keep `Cargo.lock` up to date
  and use `--locked` for checks.
- Run `script/check.sh` before handing off, or run its equivalent commands with
  your own `CARGO_TARGET_DIR`.
- This repository currently contains one Cargo package. Keep it that way until
  multiple crates are needed; if packages are added, explicitly define and
  document the Cargo workspace.
- Keep every `Cargo.toml` valid throughout edits. When creating or rewriting
  manifests under `crates/`, write the complete manifest to a temporary file
  outside `crates/`, then atomically move it into place. This prevents crate
  discovery patterns such as `crates/*` from exposing a partial manifest.
- Put new command-specific behavior in a separate module when practical; keep
  shared command dispatch and module registries small, and make surgical edits
  to files used by multiple agents.
