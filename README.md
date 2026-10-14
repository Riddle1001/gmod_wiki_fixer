# Garry's Mod Wiki Audit

A Rust CLI that audits the Facepunch Garry's Mod wiki against the `Facepunch/garrysmod` Lua repository. It produces a standalone **HTML report** with section navigation, summary metrics, searchable findings, category filters, sortable tables, and GitHub evidence pinned to the checkout's commit and exact source lines. Reports contain findings and evidence, with no proposed edits or diff blocks.

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

The report opens to **Fixes**, a focused queue of broken links, invalid source paths, and concrete API discrepancies. **Source maintenance** keeps exact line accuracy, replacement paths, and missing source/realm tags easy to reach. **Documentation opportunities** contains missing-page candidates and optional example contributions; **Audit diagnostics** contains unresolved definitions, unverified links, failed checks, and static load-path review. **All findings** retains the complete record. Findings are grouped by page in a compact queue with a separate evidence pane. Search across all page names, symbols, source paths, and finding text; narrow by category; sort by page views (highest first), page name, or category. Page views come from the fresh official `~pagelist` index, are shown on page cards, and are separate from wiki update counts. Missing or unindexed pages have unavailable views and sort after known counts. Results are paged in groups of 50 pages, with both page and finding counts shown. View, search, sort, and scope filters are retained in shareable URLs. Each selected finding keeps its original explanation, wiki/source comparison where available, and commit-pinned evidence links. Press `/` to focus search. Search remains active when changing categories; **Clear filters** returns to Fixes.

A separate **Coverage & methodology** view retains every metric, scope restriction, and interpretation limit. Missing pages, source and realm discrepancies, metadata checks, unverified links, unresolved sources, and failed checks remain distinct categories. The mobile layout uses a category selector and a full-screen evidence view. Without JavaScript, all findings remain readable as tables. Printing includes the full report, not just the current page of findings.

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

### Documentation quality

**Documentation quality** is a separate review view with 0–100 coverage ratings and poor (<50), partial (50–79), and good (80–100) bands. These bands are unvalidated heuristics, not assessments of factual accuracy. The view shows the rated count, average and distribution; guides, failed/missing pages and excluded sources are unscored. Simple, sufficient documentation with a code block/example can score 100 regardless of length.

Code/example presence supplies **60% of every function's quality score**, including simple getters and hooks. A nonempty `<example>`, `<code>`/`<pre>` block or fenced code earns 60 points; comments, inline code, empty/placeholder blocks and output alone do not count. Presence does not establish correctness or executability. The other 40% is text/metadata coverage, with relative weights purpose 40, argument types 20, complex argument guidance 10, return types 15, complex/hook return guidance 10 and realm 5, normalized over applicable criteria.

Score = round(example points + 40 x other earned / other applicable points). Every page shows raw coverage and each criterion's effective percentage. An API without code/examples scores at most 40. Longer prose earns no extra points. Simple named inputs/returns and absent argument/return lists remain exempt from those criteria; missing return tags do not mean zero returns. Source links and page length do not affect quality. This supersedes the earlier small, optional example signal at the user's request. Revisit the policy if manual reviews show that the fixed example share fails the project's contribution priorities; no project history validates these numerical weights.

Default review priority = (60 × (100 − quality) + 40 × reach) / 100. Reach = 100 × log(1 + views) / log(1 + maximum views among rated pages). Log scaling prevents one very popular page from overwhelming documentation need. Both weights are adjustable and shareable in URLs; zero/zero uses 60/40. Missing views use reach 0 and remain labeled unavailable. Normalization uses the entire rated snapshot and is independent of search or filters. Page views describe historical exposure, not a current traffic rate. Quality ratings stay out of All findings and Fixes, except text/metadata coverage at most 20 before example weighting, with no usable purpose, argument/return explanation or code/example. Missing examples alone never create a Fixes entry.

Hook/event contracts are not inferred from default callback bodies: unused arguments and optional returns are valid, and `hook.Call` can return an earlier callback's values. Source links still resolve, but signature/return comparisons and their counters omit those declarations. Supporting such comparisons requires actual dispatcher/caller contract evidence.

Discovery and source verification use the same lexical, scope-aware symbol index. Locals, function parameters, private tables and helpers, instance callbacks such as `Button.OnDrop`, ordinary data fields such as `AimAng.p`, and unresolved dynamic exports are excluded. Public exports of local functions or tables retain their public names. Registered controls, canonical metatables, inherited methods, and generic `PANEL` hooks are considered when checking existing documentation.

Existing source tags can additionally match methods installed on `self` inside a proven metatable method, the registered base `PLAYER` class, and the documented `Task`, `Schedule`, and `Stack` factory prototypes. These definitions are source-only: they do not generate missing-page suggestions or repository-wide replacement-file guesses. Factory matching requires the public constructor to return an object with the exact prototype as its `__index`; class names are explicit documented contracts, never inferred from local spelling. NextBot's `ENT` alias is restricted to the tagged base NextBot implementation. Shadowed receivers, conditional prototype assignments and ambiguous installed methods stay unresolved. Revisit these restrictions if cross-file factories or additional class registrations become supported; they require explicit ownership evidence.

AST member assignments on resolved metatable/library receivers retain their target identity and alias chains. For example, `meta.GetName = meta.Nick` matches its assignment source line and records the target as `Player:Nick`; the names are not special-cased. Unknown target signatures and native behavior remain unknown. These assignments are source-only, so copied data fields do not create missing-page suggestions. Rebound/private receivers and dynamic keys do not establish a public alias.

`gamemodes/sandbox`, `gamemodes/terrortown`, and `gamemodes/ttt` are excluded, along with `lua/menu/`, `lua/includes/menu.lua`, and `lua/includes/init_menu.lua`. Menu roots remain available to the static load graph so helpers outside those directories can be classified. Definitions guarded for Menu, and otherwise unknown definitions positively traced only from Menu, are excluded from routine coverage. Explicit client/server availability is retained even when the only observed load trace is Menu; static tracing cannot establish that other routes do not exist. The base gamemode and other Lua folders remain included. Pages whose source tags exclusively refer to excluded files are counted as skipped.

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

`.github/workflows/refresh-report.yml` runs a full audit at 17 minutes past every eighth hour in UTC, after pushes to `main`, or when started manually from the Actions tab. Manual runs on non-default branches are skipped. Each run builds the CLI, checks out a fresh shallow Garry's Mod source snapshot, fetches the current wiki index, writes `index.html`, commits that file only when it changes, and deploys a Pages artifact containing the standalone report. GitHub Actions caches Cargo downloads and wiki page bodies; the CLI validates cached page bodies against the fresh index's update counts and downloads changed pages. Configure GitHub Pages to use **GitHub Actions** as its publishing source. The workflow deploys in the same run because commits made with `GITHUB_TOKEN` do not trigger a GitHub Pages build. Revisit the single-file artifact if the report gains companion assets or if GitHub changes how token-authored commits publish Pages.

GitHub may delay scheduled runs during high load. The 17-minute offset avoids scheduling exactly on the hour.

The report shows the time the audit completed and its source revision, separately from the refresh schedule. An expandable schedule summary shows the next refresh while keeping the full clocks and countdown available. It also shows the visitor's live local time, the next scheduled refresh in local time and UTC, and a countdown. These display the schedule defined in the workflow; GitHub may start a scheduled run late, and the report changes after that run completes successfully.

## Published report

GitHub Pages serves the standalone root `index.html`. The automated workflow refreshes and deploys this snapshot; its downloaded source checkout and wiki caches stay on the temporary runner.

The prominent **Hide VGUI & panel findings** checkbox filters every review category and updates category counts without changing the underlying report. Clear filters restores these findings. Classification uses registered control names and their bases, generic Panel/PANEL and VGUI/Derma libraries, plus VGUI/Derma source evidence. Revisit this source-based registry if auditing controls loaded outside the pinned Lua snapshot becomes a supported workflow; those controls would require another explicit source of names.

Named `vgui.Register` and `derma.DefineControl` calls also supply control overview candidates, including empty controls and controls whose methods are already documented on a base. A missing overview appears under **Undocumented controls** only after the normal index, alias, redirect, and live-page checks; fetch failures stay under Failed checks. Registration lines provide commit-pinned evidence. Revisit standalone control candidates if the wiki changes its control overview address convention.

**Hide base gamemode findings** uses `gamemodes/base/` source paths across every category. It combines with the VGUI filter; Clear filters resets both.

Metatable source matching recognizes `RegisterMetaTable("PublicName", localTable)` alongside `FindMetaTable`. This preserves canonical names such as `Color:Copy` even when source methods use a private local name (`COLOR`); shadowed registration calls remain private.

## Feedback-driven scope and review policy

Known metamethods on indexed class/metatable exports do not generate standalone missing-page candidates. Existing pages still receive meaningful checks such as broken wiki references. Ordinary double-underscore functions and library helpers remain eligible. Exclusion counts are recorded in methodology; documentation opportunities do not make a source page incorrect.

Existing ENTITY and WEAPON pages can match ENT and SWEP definitions only in the explicitly tagged entity or weapon source file, with a single matching prototype definition. Repository-wide fallback does not equate those parents. Source-range and contract checks use the same contextual identity. Revisit this boundary if one wiki page must resolve inherited implementations across multiple entity/weapon owners; that needs explicit inheritance evidence rather than a global alias.

Example opportunities use actual `<example>` markup outside verbatim code. They include documented native functions, exclude hooks, metamethods, and menu-only pages, and suggest a snippet's realm, setup, expected output, and cleanup. They are optional contributions, never documentation errors. This change creates the queue; snippets are not generated or published automatically.

View membership is local to the HTML renderer's browser enhancement because it classifies the existing complete report record. Revisit if another presentation consumer needs the same view rules. The views and scope policy implement the maintainer feedback in this engagement; their usefulness has not yet been established by longer-term project history.

The CLI harness exercises contextual entity/weapon matching, ambiguous and foreign owners, menu paths/guards/load roots, retained client/menu APIs, metamethod discovery, and native/example markup cases. Browser checks cover focused views, grouping without lost findings, filters, URL restoration, mobile interactions, and complete no-JavaScript output. Accepted test sockets are made blocking explicitly on Windows before reading HTTP headers; the listener stays nonblocking to enforce the fixture's accept deadline.

### Triage report workflow

The standalone report uses a category rail, compact finding queue, and evidence pane. Top tabs choose scope; categories are sorted by count, with empty categories under “No issues.” Each finding has a shareable selection URL. Use `j` / `k` to select, `/` to search, `e` to edit on the wiki, and Escape or Back to queue to leave the mobile detail view.

Confidence describes the check's evidence, not runtime proof: definite source-path, markup, direct realm and verified link checks are Confirmed; contract, identity, missing-page and documentation heuristics are Likely; failed, unresolved, unverified-link and load-path checks are Inconclusive. The full explanation remains authoritative. Wiki/source comparison excerpts are displayed when the recorded explanation provides both values; unfamiliar wording falls back to the complete explanation.

Source excerpts come from the analyzer's immutable committed text and are limited to 80 lines per evidence link, with original line numbers and a link to the full pinned definition. They never read the mutable worktree or fetch code in the browser. Keep this within the existing report renderer while HTML is the sole presentation consumer; a second UI or upstream stable finding IDs would require revisiting browser-owned selection and identity. No project history validates a richer confidence model yet. Trend changes are explicitly unavailable until comparable prior-run data is supplied; clean source pages are labeled separately from all checked pages.
