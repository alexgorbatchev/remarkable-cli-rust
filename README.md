`remarkable` is a command-line tool for reMarkable owners and automation that browse cloud documents, inspect native handwriting, export pages, and transfer PDF documents, strokes, tags, and view settings.

# What It Does

- **Find documents:** List folders, inspect native page IDs, search PDF text, and examine hyperlinks.
- **Export pages:** Download original PDF/RM bytes, render SVG/PNG previews, or save a verified native ZIP snapshot.
- **Transfer content:** Create PDFs with recovery evidence, import native strokes into initialized pages, and transfer mapped tags and viewport settings.
- **Work offline:** Inspect/export local v6 `.rm` files and read embedded command references without cloud credentials.
- **Automate:** Use `AGENT=1` for escaped TSV, key-value lines, and explicit write states.

# How It Works

- Read the embedded skill and the reference for the command you intend to run.
- Pair with your desktop connection code for cloud operations, then identify documents by UUID, exact name, or folder path.
- Select zero-based pages for extraction/rendering; provide explicit mappings for stroke or settings transfers.
- Inspect exit status, stdout write state, and saved upload evidence before recovering a failed write.

# How it Really Works

- Credentials resolve from `--config`, `REMARKABLE_CONFIG`, an existing `$XDG_CONFIG_HOME/remarkable-cli/config.json`, then `~/.rmapi`. Pairing writes credential text with mode `0600`. Cloud operations can renew credentials.
- Downloads use hash-named files in `blobs/` beneath `--cache-dir`, `REMARKABLE_CACHE_DIR`, or `$XDG_CACHE_HOME/remarkable-cli` (`~/.cache/remarkable-cli` by default). The cache is never pruned automatically. `--no-cache` bypasses it; mutation verification downloads fresh bytes.
- Cloud operations make network requests; there is no local subscription or billing logic. Immutable blob requests recover at most three times. Root commits/authentication writes receive one transport attempt. Redirects, 90-second per-hop deadlines, command cancellation, and generation guards follow the embedded `global` reference.
- PDF operations load a SHA-256-pinned PDFium 8066 library, downloading its platform archive on first use when absent. Configure `PDFIUM_LIB_PATH`, `PDFIUM_BUNDLED_CACHE_DIR`, or `PDFIUM_NO_AUTO_DOWNLOAD` before running PDF operations. See the [render runtime](../remarkable-render/README.md#configuration) for exact cache selection.
- PDF text extraction is text-layer extraction; handwriting and images are not OCR inputs. SVG exports contain handwriting only; PNG previews composite the background and strokes. Previews do not establish tablet editability.
- `doc sync` skips any existing output path. `--force` overwrites it; a skipped file does not establish freshness. Empty, `.` and `..` display names use the document UUID as their export directory.
- Archives publish atomically into a new path after checksum and root checks. Uploads durably save credential-free recovery evidence before staging and committing. Imports preserve destination PDF/content; settings transfers preserve all files except destination content.
- Normal data and write progress go to stdout; binary `doc cat` output stays binary. Returned errors print once on stderr; `--debug` traces each physical HTTP attempt on stderr. Agent help begins with an alert to read the embedded skill.
- Exit codes are `0` success, `1` invocation/other failure, `3` missing item/file/page, `4` missing/rejected credentials, `5` network/deadline/cloud failure, and `6` an uncertain or committed write failure. Typed joined causes prefer `6`, then `4`, `5`, `3`, `1`. Status `2` is unassigned; an unrecovered Rust panic exits `101`.

# Installation

This Rust project has no published release artifact. Local build instructions are in [AGENTS.md](./AGENTS.md).

# Setup

- A reMarkable account and [desktop connection code](https://my.remarkable.com/device/desktop/connect) are required for pairing and cloud access. Supply your code to `remarkable auth pair`; keep the resulting credentials private.
- PDF operations require the [pinned native PDFium runtime](../remarkable-render/README.md#prerequisites), either cached, explicitly configured, or downloadable on first use. Local stroke/skill operations do not require PDFium.

# Quick Start

```sh
AGENT=1 remarkable skill
AGENT=1 remarkable skill reference cat documents
remarkable auth status
remarkable doc list --type DocumentType --query Daily --limit 10
remarkable doc inspect "2026 - Daily" --pages
remarkable doc cat "2026 - Daily" --page 0 --format pdf > daily.pdf
remarkable doc render "2026 - Daily" --page 0 --output page.png
```

Local fixture example from this repository:

```sh
remarkable stroke inspect tests/fixtures/sample.rm
```

Sample Output:

```text
File: tests/fixtures/sample.rm
File Size: 258 bytes
Total Blocks: 3
Lines: 2
Points: 5
Tool: Ballpoint: 1 lines
Tool: Highlighter: 1 lines
Color: Black: 1 lines
Color: Highlight: 1 lines
```

# Options & Flags

Global options apply to every command except `--version`, which applies to the root.
Boolean options accept `--flag` and `--flag=false`. Page indexes start at zero.

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--config <path>` | `-c` | `""` | Override credential file resolution. |
| `--cache-dir <path>` | — | `""` | Override download cache resolution. |
| `--debug` | — | `false` | Trace each HTTP attempt. |
| `--no-cache` | — | `false` | Bypass the download cache. |
| `--version` | `-v` | `false` | Print the raw version and newline. |
| `--help` | `-h` | `false` | Print command help. |

Each command's complete positional arguments, flag types/defaults, outputs, and side effects are embedded in exactly one reference. Run `AGENT=1 remarkable skill reference list` to select it, then `AGENT=1 remarkable skill reference cat <topic>`. These references are also readable under [skills/remarkable/references](./skills/remarkable/references).

`remarkable doc list`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--folder <id>` | — | `""` | Filter by parent folder ID. |
| `--type <type>` | — | `""` | Filter by native item type. |
| `--query <text>` | `-q` | `""` | Filter case-insensitive title substring. |
| `--limit <integer>` | — | `0` | Maximum items; nonpositive selects all. |

`remarkable doc inspect`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--pages` | — | `false` | Include native page IDs and stroke sizes. |

`remarkable doc search`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--word` | — | `false` | Match whole words. |

`remarkable doc links`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--page <index>` | — | `0` | Select the page. |

`remarkable doc cat`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--page <index>` | — | `0` | Select text/RM/SVG page; PDF streams the whole file. |
| `--format <format>` | — | `svg` | Choose `pdf`, `text`, `rm`, or `svg`. |

`remarkable doc render`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--page <index>` | — | `0` | Select the page. |
| `--dpi <integer>` | — | `200` | Resolution; nonpositive selects 200. |
| `--output <path>` | `-o` | `""` | Required PNG output; overwrite existing files. |

`remarkable doc sync`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--output-dir <path>` | `-o` | `.` | Export parent directory. |
| `--format <format>` | — | `png` | Choose `png`, `svg`, or `rm`. |
| `--dpi <integer>` | — | `200` | PNG resolution; nonpositive selects 200. |
| `--force` | `-f` | `false` | Overwrite existing page files. |

`remarkable doc archive`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--output <path>` | `-o` | `""` | Required new native ZIP path. |

`remarkable doc import`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--mapping <path>` | — | `""` | Required JSON source-file/page mapping. |

`remarkable doc upload`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--title <text>` | — | `""` | Required nonblank display name. |
| `--folder <uuid>` | — | `""` | Destination collection; empty selects root. |
| `--evidence <path>` | — | `""` | Required new recovery JSON path. |
| `--initialize-pages` | — | `false` | Initialize native identities for 1–816 pages. |

`remarkable doc settings transfer`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--mapping <path>` | — | `""` | Required JSON source/destination page mapping. |
| `--replace-viewport` | — | `false` | Permit differing destination viewport settings. |

`remarkable stroke export`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--output <path>` | `-o` | `""` | Write SVG here instead of stdout. |
| `--width <number>` | — | `0` | Canvas width when both dimensions are positive. |
| `--height <number>` | — | `0` | Canvas height when both dimensions are positive. |

`remarkable completion bash`, `remarkable completion zsh`, `remarkable completion fish`, `remarkable completion powershell`

| Flag | Short | Default | Description |
| :--- | :--- | :--- | :--- |
| `--no-descriptions` | — | `false` | Omit descriptions from generated completions. |

# Recovery and verification

Writes distinguish `staged`, `commit-unknown`, `committed`, and `verified`. Exit `6` means a root commit may have taken effect; use fresh evidence before creating another document or repeating a mutation. `doc upload-check` validates saved creation evidence without retrying creation. Command deadlines and SIGINT retain the last known write state.

The native HTTP transport does not expose most unsuccessful proxy CONNECT response statuses; those failures can exit `1` rather than `5`. DNS failures are classified through the transport's typed DNS predicate. Rasterized previews can differ from the Go renderer. Native execution is verified on Linux x86-64; cloud accounts and tablet select/move/erase behavior require separate authorized verification.

The companion [native-import workflow](./AGENTS.md#agent-driven-native-import-check) creates a disposable initialized PDF, imports selected strokes, transfers settings, and saves previews and recovery evidence. It requires Bun 1.4.2+ and explicitly authorized cloud writes. Its `--binary` option selects the executable; its default is this project's `bin/remarkable`. A passed cloud check leaves tablet verification pending until the user checks navigation, editable handwriting, background preservation, and persistence after sync.

# License

[MIT](./LICENSE), copyright 2026 Alex Gorbatchev.
