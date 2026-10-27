---
created_on: 2026-10-14 08:51
last_modified: 2026-10-14 16:00
status: current
---

# Native remarkable command contracts

This reference describes implementation boundaries and source differences for maintainers. End-user command contracts live in the canonical embedded skill and nine topic references.

## Commands and output

The command tree covers authentication/status, document listing/tree/inspection/search/links, byte extraction, PNG rendering, directory synchronization, verified ZIP archives, native stroke imports, PDF creation/recovery checks, mapped settings transfer, local stroke inspection/export, embedded skill/reference printing, root help, and Bash/Zsh/Fish/PowerShell completion.

Clap owns command parsing, boolean values, strict required options, numeric values, and shell completions. `cli-command-kit` owns human tree and agent help formatting. Application value parsers preserve the source's signed base-zero page-index syntax and file-independent validation. Root-only generated help avoids nested generated commands. Help in agent mode starts with the embedded-skill alert; version output is strictly `dev\n`. The independent Cargo package version is 0.1.0.

Shared output writes escaped agent TSV/key-values/bullet trees and human ASCII tables/tree glyphs. Returned errors print once; typed ambiguity fields produce candidate tables. Raw PDF/RM/SVG output bypasses text coercion. Native completion scripts retain the source shell command interface, while their generated content follows Clap rather than Cobra.

## Native dependencies

- Native [Clap commands](https://docs.rs/clap/latest/clap/struct.Command.html) and [clap_complete](https://docs.rs/clap_complete/latest/clap_complete/) replace Cobra/pflag and Go shell generators.
- Sibling `remarkable-cloud` supplies typed authentication, manifests/cache/resolution, HTTP recovery/redirect policy, transactional mutation snapshots, and progress callbacks.
- Sibling `rmscene` and `remarkable-render` supply binary stroke decoding/SVG and pinned PDFium PDF/text/link/PNG operations. The CLI does not invoke the Go binary or external PDF tools.
- Serde raw JSON preserves unknown tokens; exact numeric comparison uses arbitrary-precision integer coefficient/exponent normalization. ICU simple case folding implements source text search semantics. Native ZIP and adjacent temporary files implement archive/evidence publication.

## Mutations and cancellation

Archives pin a fresh root, verify manifests and all file hashes/lengths, recheck the root, and hard-link a temporary ZIP into an absent output path. An existing destination is preserved even if created concurrently.

Imports validate all mapping rows and source RM files before writes; initialized destination identities must be unique and blank apart from supported native metadata. They preserve PDF/content and update metadata `lastModified`. Settings transfers validate separate document UUIDs, mappings, raw tags, and eight typed viewport fields; they preserve source and unrelated destination bytes and freshly verify the complete result. Identical settings transfers recheck data without committing.

Uploads generate independent document/page IDs, validate PDFs, check live title/folder conflicts against fresh metadata, save private durable recovery evidence before staging, generation-check the root, and verify native associations/bytes. Initialized PDF ordering is defined for 1–816 pages; pending uploads retain null native pages. Recovery checks are read-only and retain creation revision fields.

Native cloud callbacks update external state before stage/commit/verify boundaries. Command deadlines and SIGINT drop pending HTTP futures while retaining `commit-unknown`/`committed`; runtime reports the last write state and typed exit 6. Failed state-output writes join the interruption error with the same commit precedence. Auth/pairing, reads, rendering, and mutation commands retain the source's individual total timeout values.

## Correctness differences

- Export directory sanitization falls back to the UUID for empty, `.` and `..` titles, closing the verified source traversal defect.
- Rejected HTTP response body-read failures retain typed transport causes and exit 5; the Rust implementation does not discard read failures.
- Upload file construction rejects inconsistent, duplicate, invalid, or document-equal native page IDs instead of indexing past the ordering vector. Settings services repeat identity/map validation before network access.
- Native DNS failures use reqwest 0.13.5's public [`Error::is_dns`](https://docs.rs/reqwest/latest/reqwest/struct.Error.html#method.is_dns). System DNS and proxy configuration remain native; there is no replacement resolver or message matching.
- Rust panic status is 101; status 2 remains unassigned. Parser diagnostics and shell completion source follow native Clap formatting. PNG rasterization inherits the sibling renderer's documented quantization differences.

## Limits and verification

The underlying HTTP proxy tunnel discards most unsuccessful CONNECT HTTP statuses. Their native errors can exit 1 rather than the Go CLI's 5 for CONNECT 408/429/5xx. Ordinary DNS, refused/reset connections, deadlines, incomplete bodies, and cloud statuses have typed classification. Malformed URLs and refused TLS handshakes remain exit 1.

Native PDF and stroke rendering is synchronous; a command deadline cannot preempt a synchronous native call while that call is executing. Async network requests, body buffering, retry waits, and root commits remain cancellable. PDFium platform/digest and stroke-format constraints are documented by the sibling libraries.

Behavioral tests use real loopback HTTP servers, isolated native subprocesses, malformed JSON/RM/HTTP responses, fixture PDFs, checksums, file permissions, generation conflicts, partial writes, interrupted root commits, and no-op preservation. Embedded documentation tests inspect the actual built command tree. Linux x86-64 execution is verified; live cloud and tablet editing are separate authorized checks. No local preview or successful fixture upload proves tablet editability.

The unchanged Bun companion under `scripts/native-import-check/` retains its source dependencies and lockfile. Its `--binary` option and project-relative default select `bin/remarkable`; all cloud writes go through that executable. The Just recipes build the Rust binary, install frozen dependencies with environment-file loading disabled, and retain separate type-check/check/recheck commands. The manual workflow stays outside CI and ordinary tests; see the agent instructions for source selection, uncertain-write recovery, and tablet verification.
