# Garryâ€™s Mod Wiki Audit

A Rust CLI that audits the Facepunch Garryâ€™s Mod wiki against the `Facepunch/garrysmod` Lua repository. It produces a standalone **HTML report** with section navigation, summary metrics, searchable findings, category filters, sortable tables, and GitHub evidence pinned to the checkoutâ€™s commit and exact source lines. Reports contain findings and evidence, with no proposed edits or diff blocks.

## Build and run

Install [Rust and Cargo](https://rustup.rs/) and Git, then build:

```powershell
cargo build --release
```

The CLI clones the source repository into `garrysmod_repo` if needed. Use `--pull` to update an existing checkout before auditing.

Every command analyzes Lua blobs from the checkout's recorded `HEAD` commit. Modified, deleted, staged, and untracked working-tree files are not audit inputs. This keeps source lines and GitHub evidence tied to exactly the same revision, even if the checkout changes during a run. A failed requested pull stops the command. Revisit this design only if auditing uncommitted code becomes a supported workflow; that mode needs separate local evidence rather than commit links.

```powershell
# Full wiki audit plus repository-derived missing-page candidates
.\target\release\gmod_wiki_fixer.exe audit

# Restrict coverage to a namespace or number of pages
cargo run -- audit --prefix table. --output-report table_audit.html
cargo run -- audit --limit 50 --output-report sample_audit.html

# Check one page in the terminal
cargo run -- check table.Copy

# Check addresses from a file, one per line
cargo run -- check-list test_pages.txt --output-report selected_pages.html

# Scan exported repository symbols for wiki coverage
cargo run -- scan --output-report source_coverage.html

# Update the source checkout first
cargo run -- --pull audit
```

Batch commands default to `audit_report.html`. Open the file directly in any browser; styles and scripts are embedded, with no server or external assets required.

The report opens to one review list with expandable findings. Search across all page names, symbols, source paths, and finding text; narrow by category; sort by priority, page, or category. Results are paged in groups of 50. Each expanded finding keeps its original explanation, wiki/source comparison where available, and commit-pinned evidence links. Press `/` to focus search. Search remains active when changing categories; **Clear filters** returns to all findings.

A separate **Coverage & methodology** view retains every metric, scope restriction, and interpretation limit. Missing pages, source and realm discrepancies, metadata checks, unverified links, unresolved sources, and failed checks remain distinct categories. The mobile layout uses a category selector and stacked comparisons. Without JavaScript, all findings remain readable as tables. Printing includes the full report, not just the current page of findings.

The renderer's HTML tables remain the complete record for no-JavaScript reading, printing, and `scripts/analyze_report.py`; the review list is a browser enhancement of that same record. Revisit this arrangement if another consumer needs a shared structured report format.

## What is checked

- **Source references:** missing or incorrect file tags, dead file paths, and exact function line ranges.
- **Entirely missing pages:** exported Lua APIs absent from the complete wiki index, after canonical names, inherited documentation, aliases, redirects, and fresh page checks.
- **Realm declarations:** AST guards and file conventions, plus positive evidence from known engine entry points and literal `include`, `require`, and VGUI loading paths. `AddCSLuaFile` does not count as execution.
- **Malformed source tags:** empty paths, missing or invalid line attributes, inverted or out-of-bounds ranges, unclosed tags, and duplicate tags within a function.
- **Broken wiki references:** `<page>` targets outside code examples, accounting for spelling aliases and redirects. HTTP errors and uncertain redirects are reported separately as unverified, never as confirmed missing links.
- **Function identity:** disagreement between page address, declared function name, parent, and function type, with inherited APIs and proven aliases taken into account.
- **Signature review:** argument count, varargs, and clear parameter reordering, considering implicit method `self` and source variants. Cosmetic parameter naming differences are ignored.
- **Return-value review:** impossible documented return counts and conflicting statically known types. Optional paths, unknown values, expanding calls, and uncertain control flow are handled conservatively.

## Scope and interpretation

Discovery and source verification use the same lexical, scope-aware symbol index. Locals, function parameters, private tables and helpers, instance callbacks such as `Button.OnDrop`, ordinary data fields such as `AimAng.p`, and unresolved dynamic exports are excluded. Public exports of local functions or tables retain their public names. Registered controls, canonical metatables, inherited methods, and generic `PANEL` hooks are considered when checking existing documentation.

Only `gamemodes/sandbox`, `gamemodes/terrortown`, and `gamemodes/ttt` are excluded. The base gamemode and other Lua folders remain included. Pages whose source tags exclusively refer to excluded files are counted as skipped.

Direct realm discrepancies require incompatible source and wiki availability. Positive load traces can expose additional realm availability, but cannot prove runtime behavior or the absence of other load paths; these appear as review findings. The engine model follows the official [Lua loading order](https://wiki.facepunch.com/gmod/Lua_Loading_Order), [States / Realms](https://wiki.facepunch.com/gmod/States), and [Global Variables](https://wiki.facepunch.com/gmod/Global_Variables) documentation.

The report separates clean source pages, unresolved sources, failed checks, missing-page candidates, and each finding category. Missing documentation and contract differences still need human review. No matching Lua source does not establish that a function is native. Dynamic loading and dynamic exports cannot be completely resolved by static analysis. `--prefix` and `--limit` restrict coverage.

The official `~pagelist` index supplies page addresses and update counts. All commands fetch a current index before using page caches. Cache entries are bound to the wiki origin, requested address, and update count; an index failure is reported instead of silently reusing stale data. Existing official-wiki cache files remain usable when their stored address and update count match. Targets absent from the index are freshly checked before being called missing, so repeat runs can still make reference-verification requests. `--cache-dir` selects a cache directory; `--wiki-url` selects an API-compatible origin for local fixtures or mirrors (report wiki links still target the official wiki).

Completed audits exit with status 0, including audits with findings or confirmed missing pages. Operational failures exit nonzero. Batch page failures retain a partial HTML report with failed checks listed; incomplete source reads or syntax errors stop analysis rather than pretending the repository index is complete.

## Code and generated files

`src/` holds the Rust CLI, wiki client, source indexing, audit checks, and report renderer. `src/report_assets/` holds CSS and JavaScript embedded into the standalone HTML at compile time; no frontend build step or external assets are required. Keep these assets local to the renderer while HTML is the only presentation consumer; revisit the boundary if another UI needs to share their behavior.

`scripts/` contains maintained verification and report-analysis tools. `target/`, `.cache/`, `garrysmod_repo/`, and `reports/` are generated or downloaded data and are ignored. Root scratch outputs are local; the published snapshot is `index.html`. Reports still default to `audit_report.html`; use `--output-report` to choose another existing directory. The root `analyze_worst.ps1` remains a compatibility launcher for the current HTML summary tool:

```powershell
python scripts/analyze_report.py audit_report.html --limit 15
```

## Validation

```powershell
cargo test
cargo build
python scripts/verify_cli.py --repo garrysmod_repo
```

Tests cover lexical scope, registration and inheritance, source ranges, realm guards and load paths, wiki markup, redirects and request failures, contract checks, HTML escaping, and commit-pinned evidence URLs.

The CLI harness uses temporary Git repositories and a local HTTP server, records requests and subprocess results with monotonic timestamps, and retains its JSONL log and fixtures in the printed temporary directory. It covers committed source provenance, dirty checkouts, cache freshness and identity, partial failure reports, exit status, source path confinement, invalid encoding/syntax, and pull failure. `--repo` additionally indexes a real checkout without making wiki requests.

Browser regression checks are maintained in `scripts/qa_report.py` (Python Playwright and Chromium required), using `scripts/report_browser_checks.js`. They accept populated or empty reports and cover every present category, expansion, search, sorting, pagination, keyboard access, mobile rendering, and the no-JavaScript fallback. An older `.cache/qa_report.py` copy may remain in a local workspace, but `.cache/` is ignored and is not part of the repository.

```powershell
python scripts/qa_report.py reports/wiki-audit.html
```

For synthetic reports covering every category and multiple result pages:

```powershell
$env:GMOD_REPORT_FIXTURE_DIR = ".cache/report-fixtures"
cargo test report::tests
python scripts/qa_report.py .cache/report-fixtures/populated.html
python scripts/qa_report.py .cache/report-fixtures/empty.html
```

## Automated report updates

`.github/workflows/refresh-report.yml` runs a full audit at 17 minutes past every eighth hour in UTC. It can also be started manually from the Actions tab. Each run builds the CLI, pulls the latest shallow Garry's Mod source checkout, fetches the current wiki index, writes `index.html`, commits that file only when it changes, and deploys a Pages artifact containing the standalone report. Configure GitHub Pages to use **GitHub Actions** as its publishing source. The workflow deploys in the same run because commits made with `GITHUB_TOKEN` do not trigger a GitHub Pages build. Revisit the single-file artifact if the report gains companion assets or if GitHub changes how token-authored commits publish Pages.

GitHub may delay scheduled runs during high load. The 17-minute offset avoids scheduling exactly on the hour.

## Published report

GitHub Pages serves the standalone root `index.html`. The automated workflow refreshes and deploys this snapshot; its downloaded source checkout and wiki caches stay on the temporary runner.

The prominent **Hide VGUI & panel findings** checkbox filters every review category and updates category counts without changing the underlying report. Clear filters restores these findings. Classification uses registered control names and their bases, generic Panel/PANEL and VGUI/Derma libraries, plus VGUI/Derma source evidence. Revisit this source-based registry if auditing controls loaded outside the pinned Lua snapshot becomes a supported workflow; those controls would require another explicit source of names.

Named `vgui.Register` and `derma.DefineControl` calls also supply control overview candidates, including empty controls and controls whose methods are already documented on a base. A missing overview appears under **Undocumented controls** only after the normal index, alias, redirect, and live-page checks; fetch failures stay under Failed checks. Registration lines provide commit-pinned evidence. Revisit standalone control candidates if the wiki changes its control overview address convention.

**Hide base gamemode findings** uses `gamemodes/base/` source paths across every category. It combines with the VGUI filter; Clear filters resets both.

Metatable source matching recognizes `RegisterMetaTable("PublicName", localTable)` alongside `FindMetaTable`. This preserves canonical names such as `Color:Copy` even when source methods use a private local name (`COLOR`); shadowed registration calls remain private.
