---
created_on: 2026-10-14 08:51
last_modified: 2026-10-14 16:00
status: current
---

# remarkable CLI

Independent Rust package producing `remarkable`. Shared rules in [../AGENTS.md](../AGENTS.md) apply.

## Development commands

- `just run <args>` / `just run-ai <args>` run human/agent modes.
- `just build` builds and copies the executable to `bin/remarkable`.
- `just fmt` formats only this package; `just lint` runs strict all-target Clippy.
- `just test` runs native behavioral and subprocess tests.
- `just coverage` requires at least 90% line coverage; `just check` runs formatting, Clippy, tests, and coverage.
- Development needs Rust/Cargo, `just`, `cargo-llvm-cov`, and LLVM tools. PDF tests need the Git render dependency's pinned PDFium library/cache. The embedded skill validator uses Bun with `--no-env-file`.
- `just native-import-typecheck` installs locked companion dependencies and runs `tsc --noEmit`; the manual workflow requires Bun 1.4.2+.

## Contracts and grounding

- Read the full applicable skills before edits. CLI changes require `cli-best-practices`; documentation requires `docs-writer`; embedded skill changes require `skill-writer` and its prompt audit.
- Source contracts and verified differences are in [docs/internal/contracts.md](docs/internal/contracts.md). Preserve MIT attribution and fixture provenance.
- Use Clap's native command/argument/value parsers and completion generators. Keep offline validation ahead of credentials, files, and network access. Groups show help; only root `help [command]` is generated. Commands remain subject-first and at most three levels deep.
- `runtime::execute` alone reports returned errors, once on stderr, with typed ambiguity candidates. Invocation failures show usage first; runtime failures omit it. Keep binary stdout intact, dual-mode output shared, and all output free of emojis.
- Classify errors by typed variants and source chains. Preserve joined-cause precedence and exit 6 after uncertain/committed writes. Keep external `CommitProgress` updated before root writes so dropped futures retain uncertainty; interrupted writes emit the final state.
- Cloud requests use `remarkable-cloud`'s transport policy in every mode. Preserve immutable-only recovery, exact bytes/headers, physical-attempt tracing, generation guards, fresh native checksums, and command cancellation. Never replay mutable writes on redirects or transport errors.
- Retain native schemas and unknown fields/numeric tokens. Page identities, redirections, mapping uniqueness, root stability, source preservation, and atomic no-replace archive publication are behavioral requirements.
- `skills/remarkable/SKILL.md` and its references are canonical embedded usage documents, not development instructions. Keep every command in one heading, one index row per topic in heading order, and all flags/types/defaults/outputs synchronized. Update changed metadata; keep the skill within 499 lines and each reference within 100 lines. `tests/skill.rs` checks the live command interface.

## Agent-Driven Native Import Check

- Requires Bun 1.4.2+ and cloud credentials configured for `remarkable`.
- Type-check the script: `just native-import-typecheck`.
- Run outside CI: `just native-import-check <source-uuid> --page <0-based-index> <0-based-index> [<0-based-index>...]`. After the user's tablet check and final sync, run `just native-import-recheck <evidence-dir>` with the evidence directory the check printed.
- Establish the user-designated source document and pages from available cloud data. The source must be a PDF whose native page `i` shows PDF page `i` with no deleted pages, at most 816 pages. When the user names a date, verify its PDF text and choose a matching page with pen strokes; select at least two pages, as the script requires, so imported handwriting covers several pages of the multi-page destination. Do not ask the user for a destination UUID: the script creates its own disposable document and local assets. Never substitute test fixtures or an unconfirmed source.
- The recipe builds the CLI and installs locked script dependencies. It downloads the source's PDF and the selected pages' native strokes, then creates the destination by uploading that PDF through `remarkable doc upload --initialize-pages`, which records the destination UUID and every native page ID in `creation.json` before its generation-checked root commit. Without any tablet step, it checks the page IDs with `doc inspect --pages`, imports the strokes into the same page indexes, transfers tags and view settings with an identity page map and `--replace-viewport`, verifies fresh native downloads, source preservation, unchanged page IDs, and PDF background bytes, and saves SVG/PNG previews and reports in a unique `.tmp/native-import-check/` directory. The script makes no direct cloud writes; every write goes through the CLI.
- Open the printed document title on the tablet and the printed preview paths on the computer. The script prints both 0-based CLI indexes and 1-based tablet page positions.
- The agent runs the script with stdin disabled. It exits successfully after cloud verification and leaves `tablet: pending` in the report; it never reads user input or prompts in the terminal.
- Read the exit status, `setup.json`, `creation.json`, `upload.txt`, import and settings progress, and `report.json` before claiming cloud verification passed. On failure, report the evidence directory and confirmed progress; after an uncertain upload, run `remarkable doc upload-check <creation.json>` before retrying; inspect uncertain import or settings commits before retrying them. A retry creates another document with new page IDs; retain the printed UUID for recovery and cleanup. Never write bearer tokens to evidence files.
- After cloud verification passes, give the user the exact destination title/UUID, tablet page positions, and clickable source/destination preview paths. Ask in chat whether the document opens without a crash, whether PDF navigation and links work, whether imported strokes can be selected, moved, and erased, whether the background remains intact, and whether edits persist after sync and reopening. Then run the recheck and report whether page identity `matched`.
- Keep tablet verification pending until the user explicitly reports the outcome. Record that answer separately from automated cloud verification; neither a zero exit status nor rendered previews prove tablet editability. Keep the related issue open until the required tablet check passes.
- Keep this manual check outside the CI loop. Do not write tests for this check script; use type checking and manual execution.

## Verification and boundaries

- Whenever code changes runtime behavior, change a corresponding test file and measure at least 90% line coverage (`scripts/` excluded). Record red/green and genuine temporary-disable failures, restore implementation, then rerun checks.
- Use real local HTTP servers and isolated subprocess homes. Bound waits; never infer cloud/tablet behavior from fixtures or previews. Tablet native editability remains a separate manual check.
- Never read unrelated credentials or `.env`, expose bearer tokens, make unrequested live cloud writes, or publish releases/tags/packages/deployments without explicit authorization. Keep test tokens synthetic.
- Use `rg`, never heredocs, `.tmp/` for evidence, and package-scoped formatting. Sibling path dependencies are independent repositories: never format, stage, reset, or edit them as a side effect.
- Persist standing rules here; keep task history, test results, and handoffs in `.tmp/` evidence instead.
