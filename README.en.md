# 9-max Texas Hold'em Simulation Platform

*[中文版](README.md) (default) · English*

Standalone Windows desktop application (in development). Specification documents
are listed under "Governing documents" below.

## Download a test build

Testers **do not need any development tools installed**. Grab the latest build
from [Releases](https://github.com/lovemage/taxes-leo/releases). Each release
ships three variants — pick one:

| File | Notes |
|---|---|
| `9max-sim-<version>-setup.exe` | NSIS installer (recommended) |
| `9max-sim-<version>.msi` | MSI installer, suitable for Group Policy deployment |
| `9max-sim-<version>-portable.exe` | No installation; run it directly |

> **The binary is unsigned, so SmartScreen will block it on first launch.**
> Click "More info" → "Run anyway". Tell your testers this up front — most people
> close the warning dialog and report back that "the program won't open".
>
> If the window opens but stays blank, the
> [WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/)
> is missing. Windows 10/11 normally ships with it.

Release notes live inside the app: "更新說明" in the top-right corner.

## Governing documents (these four only)

| Document | Scope |
|---|---|
| [`9max平台核心規格.md`](9max平台核心規格.md) | Authoritative baseline for product boundaries, table formats, statistics, performance |
| [`德州撲克規則細則.md`](德州撲克規則細則.md) | Game rules (mirrors real-world Texas Hold'em, TDA standard) |
| [`9max模擬平台實做計劃.md`](9max模擬平台實做計劃.md) | Milestones, schedule, staffing, risks |
| [`UI面板詳細規格.md`](UI面板詳細規格.md) | Field-level specs for panels D/F, visual design direction |

## Project layout

```
apps/
├── engine      Rust rules engine (sole authority on gameplay, zero dependencies)
├── storage     SQLite event log, RunManifest, replay data
├── ipc         Typed IPC contract (TS types are generated from here)
├── devserver   Development HTTP shell (replaced by Tauri commands in M3)
└── ui          React + TypeScript frontend
packages/
└── poker-types TS types generated from Rust structs (do not hand-edit)
```

**Game logic runs only in `engine`.** The UI never duplicates gameplay logic
(implementation plan, iron rule 6).

## Development environment

Requires Rust 1.85+, Node 22+, pnpm.

```bash
# Engine and rules tests (R1–R23 acceptance vectors)
cargo test

# Type checking and lints
cargo clippy --all-targets

# Regenerate frontend TS types from Rust structs
cargo test -p poker-ipc export_bindings
```

Line endings are always LF, pinned by [`.gitattributes`](.gitattributes). This is
not merely stylistic: Tauri's dev watcher monitors `Cargo.toml` and
`tauri.conf.json`, and a CRLF checkout on Windows makes files whose contents did
not actually change look modified, so the dev build restarts itself endlessly.

## Equity ranking asset

Every cell of preflop strategy rests on an "equity ranking of the 169 hand
classes". A content-grade ranking is a Monte Carlo run of 20,000 samples × 169
classes × 1–4 opponents: **about 5 seconds in a release build, about 80 seconds
in a debug build**.

The ranking is therefore **produced offline once** and compiled into the binary
as a versioned asset
([`apps/engine/assets/equity-rankings-v1.txt`](apps/engine/assets/equity-rankings-v1.txt),
6.6 KB); at runtime it is only parsed. No code path computes it on the fly.

```bash
# Must be re-run after changing RANKING_SEED, equity computation, or ranking rules
cargo run --release -p poker-engine --example generate_rankings

# Verify the asset really was produced with the declared seed and sample count
# (skipped by default, ~5 seconds)
cargo test --release -p poker-engine 內建資產與重算結果一致 -- --ignored
```

The asset stores only the equity of each class; ranks and percentiles are derived
by `EquityRanking::from_measurements`, which is the same code the engine uses when
computing live — so the ranges shown in a panel cannot drift from what the bots
actually play. The file carries a checksum; hand edits or corruption are caught at
load time.

**The asset and the code are one version and must be committed together.**

> Debug builds fall back to a 500-sample substitute ranking when the asset is
> unavailable, so development is not blocked. That ranking is **not content
> grade**: panel D shows a red banner and `RunManifest.equityRankingContentGrade`
> is recorded as false. Release builds have no such fallback — shipped software
> should fail loudly rather than quietly run an overnight statistics job on a
> ranking that doesn't qualify.

## Preflop default chart

Preflop content comes from the consultant's per-cell 9MAX hand chart
([`docs/9MAX手牌組合_6.xlsx`](docs/9MAX手牌組合_6.xlsx)): four effective-stack
depths × nine positions × five scenarios × five actions, 900 rows in total. It is
**content**, not parameters — each cell states outright which hands to play.

The engine has no dependencies (not even serde) and does not read Excel at runtime
(the working directory of a packaged Tauri app is not ours to decide; reading a
file would fail on the user's machine while always succeeding on a dev box). The
chart is therefore converted offline into a plain-text asset
([`apps/engine/assets/preflop-default-chart-v1.txt`](apps/engine/assets/preflop-default-chart-v1.txt),
83 KB) compiled into the binary.

```bash
# Re-run after the consultant updates the spreadsheet (needs python3 and openpyxl)
python3 tools/preflop_chart_from_xlsx.py docs/9MAX手牌組合_6.xlsx
```

The conversion first validates the source spreadsheet itself: hand-class codes are
legal, the five actions in a cell do not overlap, the "remaining hands" completion
comes to exactly 1,326 combos, and the tabulated percentages match the actual combo
counts. If any check fails it aborts without emitting a file. The asset carries a
checksum; hand edits are caught at load time.

**6–8 handed tables get no separate content.** They are derived by removing
positions: 8-handed drops UTG+2, 7-handed also drops UTG+1, 6-handed also drops LJ.
This matches the position sequence in rules §8.4.1, and the
`兩套位置序列必須一致` test pins the two together. The engine's nine stack buckets
map onto the chart's four depths; panel D's navigation labels the mapping bucket by
bucket.

Content precedence is **per-cell override → default chart → parametric generator**.
The chart has no "facing limpers" column, so those nodes still use the parametric
baseline in `BaselineRules`; the panel labels them "參數 baseline" and warns that
they are not consultant-approved.

> The chart is a pure strategy (each hand falls in exactly one action), so weight
> scaling does not affect it. The bot's `rangeWidth` / `preflopAggression` /
> `callPersistence` / `foldDiscipline` therefore act at the **content layer**:
> the most marginal hands are shifted to the adjacent action along the equity
> ordering (`default_chart::ChartShift`). The pipeline's persona stage is a no-op
> on a pure strategy, so both paths apply exactly once.

## Running the M0 vertical slice

Two processes are required:

```bash
# 1. Engine + storage + IPC (generates 500 demo hands first)
cargo run -p poker-devserver

# 2. Frontend (separate terminal)
pnpm install
pnpm --filter @taxes-leo/ui dev
```

Open http://localhost:5180. Browser mode can inspect the devserver's demo data
("重播" / Replay) and fully exercise the "Bot" and "策略" (Strategy) panels — those
endpoints are computed purely by the engine and never touch the database. Running a
simulation requires the desktop shell; commands such as `start_run` exist only on
the Tauri side.

> `devserver` is development scaffolding, not part of the product. M3 replaces it
> with Tauri commands calling the same `IpcHandler` methods; the frontend only has
> to swap the transport implementation in `apps/ui/src/api.ts`. Tauri needs
> webkit2gtk on Linux, so when a dev box lacks it this path still allows full
> development and testing.

## Running the desktop shell on Windows

The product's final form is a Tauri desktop app. **`apps/desktop` is deliberately
excluded from the Cargo workspace** (`exclude` in the root `Cargo.toml`): Tauri
needs webkit2gtk on Linux, and folding it into the workspace would make `cargo test`
fail wholesale on dev boxes that lack it. Windows uses the system's built-in
WebView2 and needs no webkit2gtk.

**One-time setup on Windows:**

1. [Rust](https://rustup.rs/) (the installer offers MSVC build tools — they are required)
2. [Node.js 22+](https://nodejs.org/) and `npm i -g pnpm`
3. WebView2 — usually built into Win10/11; otherwise install the
   [Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)

**Running:**

```powershell
git pull
pnpm install

cd apps\desktop

# Dev mode: starts Vite automatically and opens the desktop window
pnpm dev

# Stable start (release build, no file watching)
pnpm dev:stable

# Package installers (produces NSIS and MSI)
pnpm build
```

**When to use `dev:stable`** (= `tauri dev --release --no-watch`):

- `pnpm dev` is a debug build, and engine code runs **tens of times slower** than
  release. Neither batch throughput nor panel responsiveness measured on a debug
  build means anything.
- `--no-watch` disables file watching. The watcher restarts the whole build when a
  file such as `Cargo.toml` is merely touched, which is pure noise while diagnosing
  startup problems.
- The cost is a slower first build (release optimizations), but every launch after
  that is much faster.

> **Check the log first when the UI hangs.** The desktop shell writes startup, run
> start/failure, and event-dispatch failures to
> `%APPDATA%\com.zhiliu.ninemax\logs\desktop.log`. Production builds carry
> `windows_subsystem = "windows"` — there is no console, so `eprintln!` goes
> nowhere visible, and this file is the only lead.

> `pnpm dev` goes through the Tauri CLI, which reads `tauri.conf.json` and starts
> Vite via `beforeDevCommand`. Running `cargo run` directly only launches the Rust
> binary and skips the Tauri CLI's prerequisite commands. The first build compiles
> Tauri's and SQLite's C sources and takes several minutes.

> **Always mark Tauri commands `async`.** `#[tauri::command]` defaults to
> `ExecutionContext::Blocking`: the body of a synchronous command **runs directly on
> the main thread**. A single command taking a few seconds freezes the window; taking
> tens of seconds gets the process classified as `AppHangB1` by Windows. Any command
> that touches the engine or the database must be written as
> `#[tauri::command(async)]` (the function itself stays synchronous and
> `State<'_, _>` still works). The only exceptions are `pause_run` / `cancel_run` —
> they perform a single atomic store and must take effect immediately.

> **Capability changes need verification on a real machine.** Tauri v2's ACL is
> enforced only at runtime: if the frontend calls `event.listen` without the matching
> permission, TypeScript, the Rust release build, and installer packaging all pass,
> and the failure surfaces only when someone clicks it on a real machine —
> `Command plugin:event|listen not allowed by ACL`. Permissions are declared in
> [`apps/desktop/capabilities/default.json`](apps/desktop/capabilities/default.json),
> currently granting the main window only `core:event` listen/unlisten — events are
> emitted from Rust and the frontend never needs to emit. Custom commands registered
> via `generate_handler!` (`start_run` and friends) are not governed by the ACL and
> need not be listed.

**Packaging output** (measured 2026-08-27 on GitHub Actions' windows-latest):

| Target | Result |
|---|---|
| NSIS installer | 2.89 MB |
| MSI installer | 4.18 MB |
| Portable executable | 11.03 MB |

The portable build is larger because it lacks the installer's compression; all three
contain the same program.

> **The MSI requires `wix.language: "zh-TW"`.** WiX defaults to `en-US` / code page
> 1252, and the product name `9max 模擬平台` contains Chinese characters that cannot
> be encoded in that code page, so MSI packaging fails (NSIS is unaffected).
> Switching to `zh-TW` / code page 950 fixes it.
>
> If `productName` is ever changed to pure ASCII this setting stops being necessary;
> but as long as the product name keeps Chinese characters, it cannot be removed.

**How the frontend detects its environment:** `apps/ui/src/api.ts` checks whether
`window.__TAURI__` exists — if so it uses Tauri commands, otherwise it hits the
devserver's HTTP endpoints. Command names and argument shapes are identical on both
sides, so the switch happens only in that file and the rest of the frontend never has
to know where it is running.

## Building and releasing (CI)

Two workflows, deliberately kept apart: a full installer build has to compile
Tauri's and SQLite's C sources, which is too expensive to run on every PR.

| Workflow | Trigger | Contents |
|---|---|---|
| [`pr-checks.yml`](.github/workflows/pr-checks.yml) | PRs, pushes to `main` | Linux: `cargo test`, `clippy -D warnings`, frontend build, generated-types consistency; Windows: `cargo check apps/desktop` |
| [`desktop-build.yml`](.github/workflows/desktop-build.yml) | Manual, `v*` tags | Full installer packaging; a tag also opens a Release |

**Cutting a release:**

1. Update [`apps/ui/src/releaseNotes.ts`](apps/ui/src/releaseNotes.ts) (new versions
   go at the **front** of the array) and the `version` field in
   [`apps/desktop/tauri.conf.json`](apps/desktop/tauri.conf.json). The version shown
   in the header is injected by Vite from `tauri.conf.json`; the frontend does not
   keep a second copy.
2. Push the tag:

```bash
git tag -a v0.1.1 -m "..." && git push origin v0.1.1
```

Once the tag is pushed, a Release is created automatically with the three artifacts
attached. The repo is public, so testers can download without signing in to GitHub.

To get a build for testing without cutting a tag, use the manual trigger; artifacts
land in the Actions run (GitHub sign-in required, always zipped, kept 30 days).

> **The Windows job is not optional.** `apps/desktop` cannot be compiled on Linux
> (Tauri needs webkit2gtk), so locally it can only be syntax-checked. Without that
> job, type errors in Tauri commands would not surface until someone packaged an
> installer by hand.
>
> **Artifact filenames are rewritten to ASCII in CI.** `productName` is
> `9max 模擬平台`, which contains Chinese characters, so Tauri's output filenames do
> too; the download URL then gets percent-encoded and some browsers save the file
> under a mangled name.

## Player-consultant calibration workflow

The consultant **needs no development tools** — a single HTML file opened in a
browser is enough.

```bash
# Generate the interactive workbench (sliders, live preview, parameter export)
cargo run --release --example calibration_workbench
# → target/calibration-workbench.html

# Generate the read-only report (ranges only, not adjustable)
cargo run --release --example calibration_report
# → target/calibration-report.html

# After the consultant returns JSON, inspect the knock-on effects of a per-cell note
cargo run --release --example attribute_feedback
```

**How the workbench is used:** email the HTML to the consultant → they open it in a
local browser → they drag the parameter sliders on the left and the 13×13 matrix on
the right recomputes live → they click "export parameters" to download JSON → they
email it back. No server is involved at any point.

**Two lines of defence:**

1. **Drift self-validation.** The workbench's preview is recomputed in JS and could
   drift from the Rust engine. On export, Rust therefore precomputes all 1,859
   samples (11 nodes × 169 cells) and embeds them in the page; on load, JS recomputes
   them with its own implementation and compares cell by cell, showing a red warning
   at the top of the page on any mismatch. Drift is caught on the spot instead of
   silently misleading the consultant.
2. **Re-validation on read-back.** `parse_workbench_export` and `apply_import` do not
   trust what the frontend already screened; they check every value's range and
   **reject the entire batch** on any out-of-range value — a partial application
   would produce a mixed configuration nobody signed off on. After read-back
   `consultant_approved` remains `false`: adjusting parameters is not the same as
   completing sign-off.

> The workbench only handles previews. **The authoritative 687,492-cell table is
> always expanded by the Rust engine.**

## Current status

`cargo test --workspace` is fully green with 411 tests (plus one asset-recomputation
check that is skipped by default).

| Milestone | Status |
|---|---|
| M0 vertical slice | Full chain runs end to end; two hard gates unmet (see below) |
| M1 rules layer | Complete |
| M2 strategy and bot layer | Preflop uses the default chart; postflop uses a versioned equity-heuristic engineering baseline pending consultant rules |
| M3 desktop UI | V.1 app shell complete (header and status bar); panels A/B/C/E/G usable; D covers preflop only; F not built |
| M4 wrap-up and release | Windows installers can be built and released by CI; everything else not started |

**Completed**

- **M0 vertical slice**: engine → SQLite → IPC → React → per-hand replay runs end to
  end. Each hand logs 229 bytes, extrapolating to roughly 218 MB for 1M hands
  (threshold: 2 GB)
- **M1 rules layer**: all R1–R23 acceptance vectors from rules chapter 9 pass. R24
  (position labels for dead seats) and R25 (seat-count invariants) have equivalent
  tests but are not tagged with R numbers
- **M2 type skeleton**: `DecisionView` isolates hidden information at the type level
  (structurally it has no field able to carry another player's hole cards or the deck
  order), `ActionDistribution` expresses frequencies as basis-point integers, 169 hand
  classes

**The two hard M0 gates still unmet** (the implementation plan, chapter 4, lists them
as "no M1 until these pass")

1. **Measured equity time budget**: there is no equity code yet, so batch-mode
   p50 ≤1 ms / p99 ≤5 ms is unverified. If this gate falls, "1M hands ≤12 hours" is
   certain to slip in M2
2. **Content-volume production pipeline**: after precise counting the baseline is
   **727,038 cells** (`content_size` example), roughly 76 person-days to fill in by
   hand, so a parametric generator is the only option. The generator already runs
   (`generate_baseline`), but the consultant has not yet supplied the parameter values

**Other unfinished M0 freeze items**: multiway equity spike, variance-reduction spike,
258V benchmark environment. Field-level specs for panels A/C/E/G have been added to
[`UI面板詳細規格.md`](UI面板詳細規格.md).

**Current scope of panel D (own strategy)**

- Scenario navigation (table size × position × scenario × effective-stack bucket) and
  the 13×13 range matrix work. Frequencies, range widths, and raise sizes are all
  computed by the engine; the UI only draws.
- The editing path is **per-cell overrides** (`CellOverrides`): content precedence is
  per-cell override → default chart → parametric generator, and an edited cell
  overrides the other two. Overrides are attached only to the user's seat and are
  written into the `RunManifest` content snapshot along with the run.
- **The first postflop rule-node phase (UI spec D.5) is built**: flop, turn, and river each
  distinguish no-bet from facing-bet nodes and expose check, call, 1/3-pot, 2/3-pot,
  pot-sized, and fold columns. Inapplicable actions stay visible with an explanation.
  Boards use flush, flush-draw, rainbow, rainbow-paired, flush-draw-paired, trips, and the
  overlapping dry/wet tags. Consultant frequencies have not arrived, so this view is read-only.
- **The strategy library (D.9) is not built**: saving, naming, switching, and export do
  not exist yet, and overrides currently live only in the current configuration (they
  are stored in the run snapshot, but not as a separately reusable strategy file).

**Remaining M2 work**: the formal postflop strategy table (consultant content), 7 official
personas, multiway range equity, and calibration.

> **Postflop currently uses a versioned engineering baseline, not consultant-approved
> content.** `equityTexture/v2-unapproved` runs fixed-sample Monte Carlo equity against
> random legal opponent hands and buckets decisions by fair share, pot odds, and board texture.
> Active actions use exact 1/3-pot, 2/3-pot, and pot-sized options; dry boards prefer the smaller
> size while wet boards shift weight toward larger sizes. It closes
> the “everyone checks to showdown” execution gap, but reports must still label it uncalibrated.
>
> **8 of the 21 bot parameters currently change decisions.** The rest are declared
> per core spec §4.3 but the decision path does not read them yet, so
> `ParamSpec::implemented` is false and the UI lists them under "not yet in effect"
> and disables them. Tests guard both directions: marked false yet having an effect,
> and marked true yet having none, both fail the `bot_agent` test group.
