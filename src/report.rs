use crate::audit_checks::{AuditFinding, FindingKind, SourceEvidence};
use crate::diff_engine::DiscrepancyKind;
use crate::git_repo::SourceSnapshot;
use std::fmt::Write;

pub struct SourceIssue {
    pub kind: DiscrepancyKind,
    pub page: String,
    pub symbol: String,
    pub current: String,
    pub expected: String,
    pub score: usize,
    pub reason: String,
    pub evidence: Option<SourceEvidence>,
}

const SOURCE_VIEWS: [(&str, &str, &str); 6] = [
    (
        "missing-source",
        "Missing source tags",
        "Documented APIs with a resolved Lua implementation and no source tag.",
    ),
    (
        "line-ranges",
        "Line range drift",
        "Wiki source ranges that differ from the exact Lua function boundaries.",
    ),
    (
        "wrong-files",
        "Wrong file paths",
        "Source tags pointing to a different file from the resolved implementation.",
    ),
    (
        "dead-files",
        "Dead source links",
        "Tagged source files that do not exist in the checkout, with no replacement definition resolved.",
    ),
    (
        "realm-mismatches",
        "Realm mismatches",
        "Direct source evidence whose realm does not overlap the wiki declaration.",
    ),
    (
        "missing-realms",
        "Missing realm tags",
        "Resolved source definitions with realm evidence and no wiki realm declaration.",
    ),
];

fn source_category(kind: &DiscrepancyKind) -> &'static str {
    match kind {
        DiscrepancyKind::MissingSourceTag { .. } => "missing-source",
        DiscrepancyKind::LineRangeDrift => "line-ranges",
        DiscrepancyKind::WrongFile { .. } => "wrong-files",
        DiscrepancyKind::FileNotFound { .. } => "dead-files",
        DiscrepancyKind::RealmMismatch { .. } => "realm-mismatches",
        DiscrepancyKind::MissingRealmTag { .. } => "missing-realms",
    }
}

pub struct ReportData {
    pub snippets: std::collections::BTreeMap<String, serde_json::Value>,
    pub quality: Vec<crate::documentation_quality::Rating>,
    pub page_views: std::collections::BTreeMap<String, u64>,
    pub panel_names: Vec<String>,
    pub metrics: Vec<(String, usize, String)>,
    pub findings: Vec<AuditFinding>,
    pub source_issues: Vec<SourceIssue>,
    pub missing_pages: Vec<(String, Option<SourceEvidence>)>,
    pub unresolved: Vec<(String, Vec<String>)>,
    pub failed: Vec<(String, String)>,
}

pub fn wiki_url(address: &str) -> String {
    let mut url = reqwest::Url::parse("https://wiki.facepunch.com/gmod/").unwrap();
    {
        let mut path = url.path_segments_mut().unwrap();
        path.pop_if_empty();
        for segment in address.split('/') {
            path.push(segment);
        }
    }
    url.into()
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn page_link(address: &str) -> String {
    format!(
        "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{}</a>",
        escape(&wiki_url(address)),
        escape(address)
    )
}

fn sources(snapshot: &SourceSnapshot, evidence: &[SourceEvidence]) -> String {
    evidence.iter().map(|source| {
        let lines = if source.start == source.end { format!("L{}", source.start) }
            else { format!("L{}–L{}", source.start, source.end) };
        format!("<div class=\"evidence\"><a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\"><span>{}</span><code>{}:{}</code></a></div>",
            escape(&snapshot.source_url(&source.file, source.start, source.end)),
            escape(&source.label), escape(&source.file), lines)
    }).collect::<Vec<_>>().join("")
}

fn category(kind: FindingKind) -> &'static str {
    match kind {
        FindingKind::MalformedSource => "malformed-source",
        FindingKind::BrokenLink => "broken-link",
        FindingKind::UnverifiedLink => "unverified-link",
        FindingKind::IdentityConflict => "identity",
        FindingKind::SignatureDrift => "signature",
        FindingKind::ReturnDrift => "returns",
        FindingKind::ExampleOpportunity => "examples",
        FindingKind::LoadRealmConflict => "load-realm",
    }
}

fn section(
    report: &mut String,
    id: &str,
    title: &str,
    count: usize,
    description: &str,
    headers: &[&str],
) {
    write!(report, "<section id=\"{id}\" class=\"finding-section\" aria-labelledby=\"{id}-heading\"><div class=\"section-heading\"><h2 id=\"{id}-heading\">{title}</h2><span class=\"section-count\">{count}</span></div><p class=\"section-description\">{description}</p><div class=\"table-wrap\"><table class=\"findings\"><caption class=\"sr-only\">{title}</caption><thead><tr>").unwrap();
    for header in headers {
        write!(report, "<th scope=\"col\" aria-sort=\"none\"><button type=\"button\" class=\"sort\" aria-label=\"Sort by {}\">{}<span aria-hidden=\"true\"> ↕</span></button></th>", escape(header), escape(header)).unwrap();
    }
    report.push_str("</tr></thead><tbody>");
}

fn end_section(report: &mut String, count: usize) {
    report.push_str("</tbody></table></div>");
    write!(
        report,
        "<p class=\"empty-state\" {}>{}</p></section>",
        if count == 0 { "" } else { "hidden" },
        if count == 0 {
            "No findings in this section."
        } else {
            "No findings match the current filters."
        }
    )
    .unwrap();
}

pub fn render(snapshot: &SourceSnapshot, data: &ReportData) -> String {
    let metric = |name: &str| {
        data.metrics
            .iter()
            .find(|(label, _, _)| label == name)
            .map(|(_, value, _)| *value)
            .unwrap_or(0)
    };
    let load_findings = data
        .findings
        .iter()
        .filter(|f| f.kind == FindingKind::LoadRealmConflict)
        .count();
    let metadata_count = data.findings.len() - load_findings;
    let actionable = data
        .source_issues
        .iter()
        .filter(|issue| {
            matches!(
                issue.kind,
                DiscrepancyKind::FileNotFound { .. } | DiscrepancyKind::RealmMismatch { .. }
            )
        })
        .count()
        + data
            .findings
            .iter()
            .filter(|f| {
                matches!(
                    f.kind,
                    FindingKind::BrokenLink
                        | FindingKind::MalformedSource
                        | FindingKind::IdentityConflict
                        | FindingKind::SignatureDrift
                        | FindingKind::ReturnDrift
                )
            })
            .count()
        + data.quality.iter().filter(|rating| rating.critical).count();
    let mut report = String::with_capacity(32_000);
    report.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta name=\"color-scheme\" content=\"dark\"><title>Garry's Mod · Documentation review</title><style>");
    report.push_str(STYLE);
    report.push_str("</style></head><body><a class=\"skip-link\" href=\"#review\">Skip to findings</a><header class=\"masthead\"><div class=\"masthead-inner\"><a class=\"brand\" href=\"#fixes\"><span class=\"brand-mark\" aria-hidden=\"true\">g.</span><span>Garry's Mod<span class=\"brand-subtitle\">WIKI AUDIT</span></span></a><div class=\"snapshot\"><span class=\"snapshot-dot\" aria-hidden=\"true\"></span><span>Source snapshot</span>");
    write!(report, "<a href=\"{}/tree/{}\" target=\"_blank\" rel=\"noopener noreferrer\"><code>{}</code> ↗</a></div></div></header>", escape(&snapshot.github_url), escape(&snapshot.revision), escape(&snapshot.revision.chars().take(12).collect::<String>())).unwrap();
    let completed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    write!(report, "<p class=\"audit-completed shell\">Last completed audit: <time id=\"audit-completed\" data-epoch=\"{completed}\">Unix time {completed} (UTC)</time></p>").unwrap();
    report.push_str("<main class=\"shell\"><header class=\"triage-summary\"><h1>Wiki audit</h1><p>Review evidence. Fix documentation. Keep your place.</p></header><details class=\"refresh-details\"><summary>Refresh schedule <span id=\"refresh-summary\">Every 8 hours</span></summary><section class=\"refresh-panel\" aria-labelledby=\"refresh-heading\"><h2 id=\"refresh-heading\" class=\"sr-only\">Automatic report refresh</h2><div class=\"refresh-item\"><span class=\"eyebrow\">YOUR LOCAL TIME</span><time id=\"live-clock\" aria-label=\"Current local time\" aria-live=\"off\">--:--:--</time></div><div class=\"refresh-item\"><span class=\"eyebrow\">NEXT SCHEDULED REFRESH</span><time id=\"next-refresh-local\">Calculating...</time><span id=\"next-refresh-utc\" class=\"refresh-meta\">00:17, 08:17 and 16:17 UTC daily</span></div><div class=\"refresh-item\"><span class=\"eyebrow\">COUNTDOWN</span><span id=\"refresh-countdown\" class=\"refresh-countdown\" aria-live=\"off\">--:--:--</span></div><p class=\"refresh-note\">GitHub may delay scheduled starts; the report updates after a successful run.</p><noscript><p class=\"refresh-note\">The live clock and countdown require JavaScript.</p></noscript></section></details><div class=\"stats\" aria-label=\"Audit summary\">");
    for (label, count, note, href) in [
        (
            "Pages checked",
            metric("Total Pages Checked"),
            "Within this run's scope",
            "#coverage",
        ),
        (
            "Clean source pages",
            metric("Clean Source Pages"),
            "No supported checks flagged",
            "#coverage",
        ),
        (
            "Actionable findings",
            actionable,
            "Opportunities & diagnostics counted separately",
            "#fixes",
        ),
        (
            "Incomplete checks",
            data.failed.len()
                + data.unresolved.len()
                + data
                    .findings
                    .iter()
                    .filter(|f| f.kind == FindingKind::UnverifiedLink)
                    .count(),
            "Not confirmed wiki defects",
            "#diagnostics",
        ),
    ] {
        write!(report, "<a class=\"stat\" href=\"{href}\"><span>{label}</span><strong>{count}</strong><small>{note}</small></a>").unwrap();
    }
    report.push_str("</div><nav aria-label=\"Report sections\"><a class=\"tab\" id=\"review-tab\" href=\"#fixes\">Fixes <span class=\"total-badge\">");
    write!(report, "{actionable}</span></a><a class=\"tab\" id=\"coverage-tab\" href=\"#coverage\">Coverage & methodology</a><a class=\"tab\" href=\"#maintenance\">Source maintenance</a><a class=\"tab\" href=\"#opportunities\">Contribute</a><a class=\"tab\" href=\"#diagnostics\">Diagnostics</a><span class=\"nav-note\">Read-only report · commit-pinned evidence</span></nav>").unwrap();
    // Tables remain the canonical rendered record for no-JS reading, printing and
    // the HTML summary tool. JS enhances that record into a paged review list.
    // Revisit if another consumer needs a shared structured report format.
    report.push_str(r##"<div id="review" class="workbench enhanced-only"><aside class="filters" aria-label="Finding categories"><div class="filter-heading"><span>REVIEW QUEUE</span><span id="queue-total"></span></div><div id="category-list"></div><div class="rail-note"><strong>Evidence, not automatic edits.</strong><p>Contract and realm findings need human judgment. Uncertain checks stay visible.</p><a href="#coverage">See scope & limitations →</a></div></aside><div class="queue"><div class="queue-heading"><div><p class="eyebrow" id="queue-eyebrow">THE COMPLETE PICTURE</p><h2 id="queue-title">All findings</h2></div><button type="button" id="expand-all" aria-pressed="false">Expand this page</button></div><p id="queue-description">All findings and incomplete checks, with their original evidence.</p><div class="queue-tools"><label class="search-box"><span class="sr-only">Search all findings</span><svg aria-hidden="true" viewBox="0 0 24 24"><circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/></svg><input type="search" id="search" placeholder="Search pages, symbols, files…" autocomplete="off"><kbd aria-hidden="true">/</kbd></label><label class="sort-box"><span class="sr-only">Sort findings</span><select id="sort"><option value="priority">Page views — highest first</option><option value="page">Page name · A–Z</option><option value="category">Category · A–Z</option></select></label></div><section id="quality-controls" class="quality-controls" hidden aria-label="Documentation scoring"><p id="quality-summary"></p><div class="quality-weights"><label>Documentation need weight <input id="need-weight" type="range" min="0" max="100" value="60"><output id="need-weight-value">60</output></label><label>Page views weight <input id="views-weight" type="range" min="0" max="100" value="40"><output id="views-weight-value">40</output></label><label>Show <select id="quality-band"><option value="improve">Room to improve (&lt;100)</option><option value="all">All rated pages</option><option value="poor">Poor coverage (&lt;50)</option><option value="partial">Partial coverage (50–79)</option><option value="good">Good coverage (80–100)</option></select></label></div><p id="quality-formula"></p><details><summary>How scores and priorities work</summary><p>This is a heuristic of visible documentation coverage, not an assessment of truth or writing quality. A short getter can score 100. No source link, prose length, or default hook body affects this score. Missing return tags do not establish zero returns. Unrated pages are not counted as good.</p><p>Applicable weights: purpose 40, argument types 20, complex argument guidance 10, return types 15, complex/hook return guidance 10, realm 5, example support 5. Each row shows earned/applicable points. Score = round(100 × total earned / total applicable). Simple arguments/returns, hooks needing no example, and absent argument/return lists do not incur those penalties. Examples are optional and contribute at most 5 points. All declared overloads are assessed.</p><p>Priority = (need weight × (100 − quality) + views weight × reach) / total weight. Reach = 100 × log(1 + views) / log(1 + maximum views among all rated pages in this report). Logarithms reduce the dominance of extremely popular pages; page views indicate historical exposure, not current traffic. Missing views use reach 0 and are labeled unavailable. Search and filters do not change normalization. If both weights are zero, 60/40 applies.</p><p>Scores below 50 mean poor coverage, 50–79 partial, and 80–100 good coverage. These are review bands, not validated quality judgments. Only a score at most 20 with no usable purpose, argument/return explanation or example appears in Fixes; a low score alone does not.</p></details></section><label class="panel-filter"><input type="checkbox" id="hide-panels"><span><strong>Hide VGUI &amp; panel findings</strong><small>Across every category: controls, panel methods, VGUI and Derma APIs.</small></span><span id="panel-filter-count" class="panel-filter-count"></span></label><label class="panel-filter"><input type="checkbox" id="hide-base"><span><strong>Hide base gamemode findings</strong><small>Across every category: findings with gamemodes/base source evidence.</small></span><span id="base-filter-count" class="panel-filter-count"></span></label><div class="mobile-filter"><label for="category-picker">Category</label><select id="category-picker"></select></div><div class="results-line"><span id="result-count" role="status" aria-live="polite"></span><button type="button" id="reset">Clear filters</button></div><div id="finding-list"></div><div id="empty-state" hidden><span class="empty-mark" aria-hidden="true">?</span><h3>No findings here.</h3><p>Choose another category or clear your search.</p></div><div class="pagination"><button type="button" id="previous">← Previous</button><span id="page-position" aria-live="polite"></span><button type="button" id="next">Next →</button></div></div></div><section id="coverage" aria-labelledby="coverage-heading"><div class="coverage-intro"><p class="eyebrow">HOW TO READ THIS REPORT</p><h2 id="coverage-heading">The boundaries of the audit.</h2><p>Findings are evidence for review. A static check can identify a discrepancy without proving what happens at runtime.</p></div><div class="method-grid"><article><span class="method-number">01 / SOURCE</span><h3>A fixed point of reference.</h3><p>GitHub evidence points to the recorded commit and exact source lines. Uncommitted changes are not included.</p></article><article><span class="method-number">02 / SCOPE</span><h3>Public Lua, with limits.</h3><p>Sandbox, TTT, and menu-only APIs are excluded; the base gamemode and other Lua files remain included. Missing-page candidates exclude metamethods, private helpers, locals, instance callbacks, ordinary fields, and unresolved dynamic exports. Canonical and inherited documentation covers overrides. Prefix and limit options restrict coverage.</p></article><article><span class="method-number">03 / INTERPRETATION</span><h3>Uncertainty stays visible.</h3><p>Contracts account for method self, varargs, overloads, and optional returns. Dynamic returns and uncertain control flow remain inconclusive. Realm traces establish static load paths, not runtime behavior or the absence of other routes. Failed and unresolved checks are never clean results.</p></article></div><p class="reference-note">Engine roots and realm constants follow the official <a href="https://wiki.facepunch.com/gmod/Lua_Loading_Order" target="_blank" rel="noopener noreferrer">Lua loading order</a>, <a href="https://wiki.facepunch.com/gmod/States" target="_blank" rel="noopener noreferrer">States / Realms</a>, and <a href="https://wiki.facepunch.com/gmod/Global_Variables" target="_blank" rel="noopener noreferrer">Global Variables</a> references.</p><h3 class="metrics-title">All metrics & coverage</h3><div class="table-wrap"><table class="coverage"><caption class="sr-only">All audit metrics</caption><thead><tr><th scope="col">Metric</th><th scope="col">Count</th><th scope="col">Meaning</th></tr></thead><tbody>"##);
    for (label, count, description) in &data.metrics {
        write!(
            report,
            "<tr><th scope=\"row\">{}</th><td class=\"number\">{count}</td><td>{}</td></tr>",
            escape(label),
            escape(description)
        )
        .unwrap();
    }
    for kind in FindingKind::ALL {
        write!(
            report,
            "<tr><th scope=\"row\">{}</th><td class=\"number\">{}</td><td>{}</td></tr>",
            escape(kind.label()),
            data.findings.iter().filter(|f| f.kind == kind).count(),
            match kind {
                FindingKind::UnverifiedLink =>
                    "Request or redirect uncertainty; never counted as a broken link",
                FindingKind::BrokenLink =>
                    "Target absent after index, alias, redirect, and live-page verification",
                _ => "Individual evidence findings for review",
            }
        )
        .unwrap();
    }
    report.push_str("</tbody></table></div></section><div id=\"source-data\"><noscript><p class=\"notice\">JavaScript is off. Every finding and evidence link is available in the tables below. Enable JavaScript for search, filters, sorting, and the review list.</p></noscript>");

    section(
        &mut report,
        "missing-pages",
        "Missing page candidates",
        data.missing_pages.len(),
        "Exported Lua APIs with no matching wiki page after alias, inheritance, redirect, and live-page checks. Undocumented status still warrants review.",
        &["Page", "Repository evidence"],
    );
    let mut missing: Vec<_> = data.missing_pages.iter().collect();
    missing.sort_by(|a, b| a.0.cmp(&b.0));
    for (address, evidence) in missing {
        write!(
            report,
            "<tr data-category=\"{}\"><td>{}</td><td>{}</td></tr>",
            if data.panel_names.contains(address) {
                "missing-controls"
            } else {
                "missing"
            },
            page_link(address),
            evidence
                .as_ref()
                .map(|source| sources(snapshot, std::slice::from_ref(source)))
                .unwrap_or_else(
                    || "<span class=\"muted\">No exported Lua definition found</span>".into()
                )
        )
        .unwrap();
    }
    end_section(&mut report, data.missing_pages.len());

    for (id, title, description) in SOURCE_VIEWS {
        let count = data
            .source_issues
            .iter()
            .filter(|issue| source_category(&issue.kind) == id)
            .count();
        section(
            &mut report,
            id,
            title,
            count,
            description,
            &[
                "Page views",
                "Page / symbol",
                "Wiki value",
                "Source evidence / expected",
                "Finding",
            ],
        );
        let mut issues: Vec<_> = data
            .source_issues
            .iter()
            .filter(|issue| source_category(&issue.kind) == id)
            .collect();
        issues.sort_by(|a, b| {
            data.page_views
                .get(&b.page)
                .cmp(&data.page_views.get(&a.page))
                .then(a.page.cmp(&b.page))
        });
        for issue in issues {
            write!(report, "<tr data-category=\"{id}\" data-severity-score=\"{}\"><td class=\"number\">{}</td><td>{}<code class=\"symbol\">{}</code></td><td><code>{}</code></td><td>{}<code class=\"expected\">{}</code></td><td>{}</td></tr>",
            issue.score, data.page_views.get(&issue.page).map(u64::to_string).unwrap_or_else(|| "Unavailable".into()), page_link(&issue.page), escape(&issue.symbol), escape(&issue.current),
            issue.evidence.as_ref().map(|source| sources(snapshot, std::slice::from_ref(source))).unwrap_or_default(),
            escape(&issue.expected), escape(&issue.reason)).unwrap();
        }
        end_section(&mut report, count);
    }

    for (id, title, description, load_only) in [
        (
            "metadata",
            "Metadata & link findings",
            "Structural checks, reference verification, API identity, and function contracts. Unverified links remain separate from confirmed broken links.",
            false,
        ),
        (
            "load-realms",
            "Load path realm review",
            "Positive engine/include paths reach the definition in additional realms. Runtime dependencies and dynamic loading remain unproven; these findings require review.",
            true,
        ),
    ] {
        let count = if load_only {
            load_findings
        } else {
            metadata_count
        };
        section(
            &mut report,
            id,
            title,
            count,
            description,
            &["Category", "Page", "Finding", "Evidence"],
        );
        let mut findings: Vec<_> = data
            .findings
            .iter()
            .filter(|finding| (finding.kind == FindingKind::LoadRealmConflict) == load_only)
            .collect();
        findings.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then(a.page.cmp(&b.page))
                .then(a.message.cmp(&b.message))
        });
        for finding in findings {
            let mut evidence = sources(snapshot, &finding.sources);
            if let Some(line) = finding.wiki_line {
                write!(
                    evidence,
                    "<span class=\"wiki-line\">Wiki markup line {line}</span>"
                )
                .unwrap();
            }
            if let Some(target) = &finding.target {
                write!(evidence, "<div>Target: {}</div>", page_link(target)).unwrap();
            }
            write!(report, "<tr data-category=\"{}\"><td><span class=\"tag\">{}</span></td><td>{}</td><td>{}</td><td>{}</td></tr>",
            category(finding.kind), escape(finding.kind.label()), page_link(&finding.page), escape(&finding.message), evidence).unwrap();
        }
        end_section(&mut report, count);
    }

    section(
        &mut report,
        "unresolved",
        "Unresolved source definitions",
        data.unresolved.len(),
        "Tagged symbols that could not be resolved confidently. These pages are not counted as clean.",
        &["Page", "Unresolved evidence"],
    );
    for (page, messages) in &data.unresolved {
        write!(
            report,
            "<tr data-category=\"unresolved\"><td>{}</td><td>{}</td></tr>",
            page_link(page),
            escape(&messages.join("; "))
        )
        .unwrap();
    }
    end_section(&mut report, data.unresolved.len());
    section(
        &mut report,
        "failed",
        "Failed checks",
        data.failed.len(),
        "Incomplete checks caused by fetch, parse, or resolution errors. These pages are never counted as missing or clean.",
        &["Page", "Error"],
    );
    for (page, error) in &data.failed {
        write!(
            report,
            "<tr data-category=\"failed\"><td>{}</td><td>{}</td></tr>",
            page_link(page),
            escape(error)
        )
        .unwrap();
    }
    end_section(&mut report, data.failed.len());
    let critical: Vec<_> = data
        .quality
        .iter()
        .filter(|rating| rating.critical)
        .collect();
    section(
        &mut report,
        "doc-critical",
        "Empty documentation",
        critical.len(),
        "Only scores at most 20 with no usable purpose, argument/return explanation or example. Low scores alone stay in Documentation quality.",
        &["Page", "Finding"],
    );
    for rating in &critical {
        write!(report, "<tr data-category=\"doc-critical\"><td>{}</td><td>No usable purpose or argument/return guidance; documentation coverage {}/100. Review this empty documentation.</td></tr>", page_link(&rating.page), rating.score).unwrap();
    }
    end_section(&mut report, critical.len());
    report.push_str("</div><div id=\"quality-data\">");
    section(
        &mut report,
        "documentation-quality",
        "Documentation quality",
        data.quality.len(),
        "Heuristic coverage scores, not proof of accuracy. 100 is complete applicable coverage; 0 is no coverage. Short sufficient documentation can score 100. Guides, missing/failed pages, and excluded sources are not rated.",
        &["Quality", "Page", "Assessment", "Score breakdown"],
    );
    for rating in &data.quality {
        let band = if rating.score < 50 {
            "Poor coverage"
        } else if rating.score < 80 {
            "Partial coverage"
        } else {
            "Good coverage"
        };
        let missing: Vec<_> = rating
            .criteria
            .iter()
            .filter(|c| c.earned < c.possible)
            .map(|c| c.name)
            .collect();
        let assessment = if missing.is_empty() {
            "All applicable signals covered".into()
        } else {
            format!("Review: {}", missing.join(", "))
        };
        write!(report, "<tr data-category=\"documentation-quality\" data-quality=\"{}\" data-base=\"{}\" data-panel=\"{}\"><td>{}/100</td><td>{}</td><td>{}: {}</td><td><ul class=\"quality-breakdown\">", rating.score,
            rating.sources.iter().any(|p| p.starts_with("gamemodes/base/")),
            rating.sources.iter().any(|p| p.contains("/vgui/") || p.contains("/derma/")),
            rating.score, page_link(&rating.page), band, escape(&assessment)).unwrap();
        for criterion in &rating.criteria {
            write!(
                report,
                "<li><strong>{}: {}/{}</strong> — {}</li>",
                escape(criterion.name),
                criterion.earned,
                criterion.possible,
                escape(criterion.explanation)
            )
            .unwrap();
        }
        report.push_str("</ul><p>Score = rounded 100 × earned points / applicable points. Nonapplicable criteria are omitted. This measures visible guidance, not factual correctness or prose length.</p></td></tr>");
    }
    end_section(&mut report, data.quality.len());
    report.push_str("</div><footer><span>Garry's Mod <strong>Wiki Audit</strong></span><span>Static analysis. Evidence for human review.</span></footer></main><script>");
    // Emit the snapshot registry, not a spelling heuristic (many controls lack a D prefix).
    let names = serde_json::to_string(&data.panel_names)
        .unwrap()
        .replace('<', "\\u003c");
    write!(report, "const registeredPanelNames = {names};\n").unwrap();
    let views: std::collections::BTreeMap<_, _> = data
        .page_views
        .iter()
        .map(|(page, count)| (page, count.to_string()))
        .collect();
    let views = serde_json::to_string(&views)
        .unwrap()
        .replace('<', "\\u003c");
    write!(report, "const pageViews = {views};\n").unwrap();
    write!(
        report,
        "const sourceSnippets = {};",
        serde_json::to_string(&data.snippets)
            .unwrap()
            .replace('<', "\\u003c")
    )
    .unwrap();
    report.push_str(SCRIPT);
    report.push_str("</script></body></html>");
    report
}

const STYLE: &str = include_str!("report_assets/style.css");

const SCRIPT: &str = include_str!("report_assets/report.js");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_preserves_all_finding_families_and_empty_coverage() {
        let snapshot = SourceSnapshot {
            revision: "0123456789012345678901234567890123456789".into(),
            github_url: "https://github.com/Facepunch/garrysmod".into(),
        };
        let evidence = SourceEvidence {
            file: "lua/includes/example.lua".into(),
            start: 12,
            end: 24,
            label: "Source definition".into(),
        };
        let mut data = ReportData {
        snippets: Default::default(),
            quality: vec![
                crate::documentation_quality::rate("Global.QualityGood", "<function name='QualityGood'><description>Returns time.</description><realm>Shared</realm><rets><ret type='number'/></rets></function>").unwrap(),
                crate::documentation_quality::rate("Global.QualityPoor", "<function name='QualityPoor'></function>").unwrap(),
                crate::documentation_quality::rate("Global.QualityPartial", "<function name='QualityPartial'><description>Registers data.</description><args><arg name='data' type='table'/></args></function>").unwrap(),
            ],
            page_views: std::collections::BTreeMap::new(),
            panel_names: vec!["SpawnIcon".into(), "ImageCheckBox".into()],
            metrics: vec![(
                "Total Pages Checked".into(),
                85,
                "Synthetic browser regression fixture".into(),
            )],
            findings: vec![],
            source_issues: vec![],
            missing_pages: vec![("Global.Undocumented".into(), Some(evidence.clone()))],
            unresolved: vec![(
                "Global.Unresolved".into(),
                vec!["No confident match was found".into()],
            )],
            failed: vec![(
                "Global.Failed".into(),
                "HTTP 503; not a missing page".into(),
            )],
        };
        let kinds = [
            DiscrepancyKind::LineRangeDrift,
            DiscrepancyKind::MissingSourceTag {
                target_file: evidence.file.clone(),
            },
            DiscrepancyKind::WrongFile {
                original_file: "lua/old.lua".into(),
                target_file: evidence.file.clone(),
            },
            DiscrepancyKind::FileNotFound {
                original_file: "lua/deleted.lua".into(),
            },
            DiscrepancyKind::RealmMismatch {
                wiki_realm: "Client".into(),
                repo_realm: "Server".into(),
            },
            DiscrepancyKind::MissingRealmTag {
                repo_realm: "Shared".into(),
            },
        ];
        for i in 0..67 {
            data.source_issues.push(SourceIssue { kind: kinds[i % kinds.len()].clone(), page: format!("Example.Function{i:03}"), symbol: format!("Example.Function{i:03}"), current: "4-L9".into(), expected: "12-L24".into(), score: i * 100, reason: "Review this finding against the recorded source. A longer explanation remains available in full when expanded.".into(), evidence: Some(evidence.clone()) });
        }
        for kind in FindingKind::ALL {
            let mut finding = AuditFinding::new(
                kind,
                "Example.AVeryLongSymbolNameThatMustWrapOnNarrowScreensAndNeverOverflowTheReport",
                "Review <script>alert('unsafe')</script> & preserve all original information."
                    .into(),
            );
            finding.sources = vec![evidence.clone()];
            finding.wiki_line = Some(8);
            finding.target = Some("Global.ReferenceTarget".into());
            data.findings.push(finding);
        }
        for i in 0..65 {
            data.page_views.insert(
                format!("Example.Function{i:03}"),
                ((i * 17) % 65) as u64 * 100,
            );
        }
        data.page_views.insert(
            "Example.AVeryLongSymbolNameThatMustWrapOnNarrowScreensAndNeverOverflowTheReport"
                .into(),
            9999,
        );
        data.page_views.insert("Global.QualityGood".into(), 10000);
        data.page_views.insert("Global.QualityPoor".into(), 5);
        data.page_views.insert("Global.QualityPartial".into(), 1000);
        let full = render(&snapshot, &data);
        assert_eq!(full.matches("<tr data-category=").count(), 82);
        for kind in FindingKind::ALL {
            assert!(full.contains(kind.label()));
        }
        assert!(full.contains("HTTP 503; not a missing page"));
        assert!(full.contains("&lt;script&gt;alert"));
        data.source_issues.clear();
        data.findings.clear();
        data.missing_pages.clear();
        data.unresolved.clear();
        data.failed.clear();
        data.quality.clear();
        let empty = render(&snapshot, &data);
        assert_eq!(empty.matches("<tr data-category=").count(), 0);
        // Optional fixture export drives the browser through the real renderer.
        if let Some(dir) = std::env::var_os("GMOD_REPORT_FIXTURE_DIR") {
            let dir = std::path::PathBuf::from(dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("populated.html"), full).unwrap();
            std::fs::write(dir.join("empty.html"), empty).unwrap();
        }
    }

    #[test]
    fn standalone_html_escapes_wiki_data_and_links_exact_evidence_without_edits() {
        let snapshot = SourceSnapshot {
            revision: "0123456789012345678901234567890123456789".into(),
            github_url: "https://github.com/Facepunch/garrysmod".into(),
        };
        let mut finding = AuditFinding::new(
            FindingKind::SignatureDrift,
            "api.Test",
            "Review <script>alert('x')</script> | y".into(),
        );
        finding.sources.push(SourceEvidence {
            file: "lua/test.lua".into(),
            start: 2,
            end: 7,
            label: "api.Test".into(),
        });
        let data = ReportData {
            snippets: Default::default(),
            quality: Vec::new(),
            page_views: std::collections::BTreeMap::new(),
            panel_names: vec![],
            metrics: vec![("Total Pages Checked".into(), 1, "Pages".into())],
            findings: vec![finding],
            source_issues: vec![],
            missing_pages: vec![("api.<Missing>".into(), None)],
            unresolved: vec![],
            failed: vec![],
        };
        let report = render(&snapshot, &data);
        assert!(report.starts_with("<!doctype html>"));
        assert!(report.contains("<nav aria-label=\"Report sections\">"));
        assert!(report.contains("Signature Review Findings"));
        assert!(report.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; | y"));
        assert!(report.contains("api.&lt;Missing&gt;"));
        assert!(report.contains(
            "/blob/0123456789012345678901234567890123456789/garrysmod/lua/test.lua#L2-L7"
        ));
        assert!(!report.contains("<script>alert"));
        assert!(!report.contains("```"));
        assert!(!report.contains("Unified Diff"));
        assert!(!report.contains("New tag:"));
    }
}
