use clap::{Parser, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};

mod analyzer;
mod ast_indexer;
mod audit_checks;
mod diff_engine;
mod documentation_quality;
mod function_contract;
mod git_repo;
mod load_graph;
mod report;
mod symbol_indexer;
mod wiki_client;
mod wiki_markup;

use analyzer::{Analyzer, CheckPageOutcome};
use diff_engine::{Discrepancy, DiscrepancyKind};
use git_repo::GmodRepo;
use wiki_client::WikiClient;

#[derive(Parser)]
#[command(name = "gmod_wiki_fixer")]
#[command(about = "Audit the Garry's Mod wiki against committed Lua sources", long_about = None)]
struct Cli {
    #[arg(short, long, default_value = "./garrysmod_repo")]
    repo_path: PathBuf,

    #[arg(long, default_value_t = false)]
    pull: bool,

    /// Wiki origin (also supports a local fixture server or mirror).
    #[arg(long, default_value = "https://wiki.facepunch.com")]
    wiki_url: String,

    #[arg(long, default_value = "./.cache/pages")]
    cache_dir: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check a single wiki page (e.g. `check table.Copy` or `check Global.Color`)
    Check { page: String },
    /// Check a batch of pages read from a file (one address per line)
    CheckList {
        file: PathBuf,

        #[arg(short, long)]
        output_report: Option<PathBuf>,
    },
    /// Discover symbols from the Garry's Mod repository and check their wiki pages
    Scan {
        #[arg(short, long)]
        limit: Option<usize>,

        #[arg(short, long)]
        output_report: Option<PathBuf>,
    },
    /// Audit official wiki pages from ~pagelist against the repository
    Audit {
        #[arg(short, long)]
        prefix: Option<String>,

        #[arg(short, long)]
        limit: Option<usize>,

        #[arg(short, long)]
        output_report: Option<PathBuf>,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    println!("========================================================");
    println!(" Garry's Mod Wiki Audit (Rust AST Engine) ");
    println!("========================================================\n");

    let repo = GmodRepo::ensure_repo(&cli.repo_path, cli.pull)
        .map_err(|e| format!("Failed to initialize repository: {}", e))?;

    let snapshot = repo.source_snapshot()?;
    println!(
        "Analyzing committed source revision {} (working-tree edits are not included)",
        snapshot.revision
    );
    let wiki_client = WikiClient::new(&cli.wiki_url, &cli.cache_dir);
    let mut analyzer = Analyzer::from_snapshot(repo, wiki_client, &snapshot)?;

    match cli.command {
        Commands::Check { page } => {
            println!("Checking wiki page: {}...", page);
            match analyzer.check_page(&page) {
                Ok(CheckPageOutcome::ValidSource { result }) => {
                    for finding in &result.findings {
                        println!("  {}: {}", finding.kind.label(), finding.message);
                    }
                    if !result.unresolved_sources.is_empty() {
                        println!("Source verification requires review:");
                        for source in &result.unresolved_sources {
                            println!("  {}", source);
                        }
                    }
                    if result.discrepancies.is_empty()
                        && result.unresolved_sources.is_empty()
                        && result.findings.is_empty()
                    {
                        println!(
                            "✅ No discrepancies detected within supported source and realm checks.\n"
                        );
                    } else if !result.discrepancies.is_empty() {
                        println!(
                            "❌ Found {} discrepancy/discrepancies!\n",
                            result.discrepancies.len()
                        );
                        for (i, d) in result.discrepancies.iter().enumerate() {
                            println!("  [Discrepancy #{}]", i + 1);
                            println!("  Symbol:   {}", d.symbol_name);
                            println!("  File:     {}", d.file_path);
                            match &d.kind {
                                DiscrepancyKind::LineRangeDrift => {
                                    println!("  Kind:     LINE RANGE DRIFT");
                                }
                                DiscrepancyKind::WrongFile {
                                    original_file,
                                    target_file,
                                } => {
                                    println!("  Kind:     WRONG FILE (Moved/Mispointed)");
                                    println!("  Old File: {}", original_file);
                                    println!("  New File: {}", target_file);
                                }
                                DiscrepancyKind::MissingSourceTag { target_file } => {
                                    println!(
                                        "  Kind:     MISSING SOURCE TAG (Implemented in Lua, not native C!)"
                                    );
                                    println!("  Target:   {}", target_file);
                                }
                                DiscrepancyKind::FileNotFound { original_file } => {
                                    println!("  Kind:     FILE NOT FOUND (Dead source link)");
                                    println!("  File:     {}", original_file);
                                }
                                DiscrepancyKind::RealmMismatch {
                                    wiki_realm,
                                    repo_realm,
                                } => {
                                    println!("  Kind:     REALM MISMATCH");
                                    println!("  Wiki:     {}", wiki_realm);
                                    println!("  Repo:     {}", repo_realm);
                                }
                                DiscrepancyKind::MissingRealmTag { repo_realm } => {
                                    println!("  Kind:     MISSING REALM TAG");
                                    println!("  Repo:     {}", repo_realm);
                                }
                            }
                            println!("  Current:  {}", d.current_line_attr);
                            println!(
                                "  Expected: {} (lines {}-{})",
                                d.expected_line_attr, d.expected_start_line, d.expected_end_line
                            );
                            println!();
                        }
                    }
                }
                Ok(CheckPageOutcome::NoMatchingLuaSource { title, findings }) => {
                    for finding in &findings {
                        println!("  {}: {}", finding.kind.label(), finding.message);
                    }
                    println!(
                        "No matching Lua source; native status is unverified (Title: {}).\n",
                        title
                    );
                }
                Ok(CheckPageOutcome::ExcludedSource { title }) => {
                    println!("Skipped excluded Sandbox/TTT source page: {}", title);
                }
                Ok(CheckPageOutcome::NotFound) => {
                    println!("⚠️  Page not found on Facepunch wiki (404).\n");
                }
                Err(e) => {
                    return Err(format!("Error checking page {page}: {e}").into());
                }
            }
        }

        Commands::CheckList {
            file,
            output_report,
        } => {
            let content = fs::read_to_string(&file)?;
            let pages: Vec<String> = content
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect();

            println!(
                "\nChecking {} pages from {}...\n",
                pages.len(),
                file.display()
            );
            let default_report = PathBuf::from("audit_report.html");
            let rep_path = output_report.as_ref().unwrap_or(&default_report);
            run_batch_check(&mut analyzer, &snapshot, &pages, Some(rep_path))?;
        }

        Commands::Scan {
            limit,
            output_report,
        } => {
            println!("\nScanning Lua files in repository to discover symbol pages...");
            let mut pages = analyzer
                .scan_repo_symbols()
                .map_err(|e| format!("Scan error: {}", e))?;

            println!(
                "Discovered {} unique symbol pages from codebase.",
                pages.len()
            );

            if let Some(lim) = limit {
                pages.truncate(lim);
                println!("Limiting check to {} pages.", pages.len());
            }

            let default_report = PathBuf::from("audit_report.html");
            let rep_path = output_report.as_ref().unwrap_or(&default_report);
            run_batch_check(&mut analyzer, &snapshot, &pages, Some(rep_path))?;
        }

        Commands::Audit {
            prefix,
            limit,
            output_report,
        } => {
            println!("\nSyncing with official Facepunch ~pagelist...");
            let mut candidates = analyzer
                .get_candidate_pagelist_pages(prefix.as_deref())
                .map_err(|e| format!("Pagelist error: {}", e))?;

            println!(
                "Identified {} candidate GLua pages for audit.",
                candidates.len()
            );

            if let Some(lim) = limit {
                candidates.truncate(lim);
                println!("Limiting audit to {} pages.", candidates.len());
            }

            let default_report = PathBuf::from("audit_report.html");
            let rep_path = output_report.as_ref().unwrap_or(&default_report);
            run_batch_check(&mut analyzer, &snapshot, &candidates, Some(rep_path))?;
        }
    }

    Ok(())
}

fn parse_line_span(attr: &str) -> (usize, usize) {
    let clean = attr.trim();
    let parts: Vec<&str> = clean
        .split(|c: char| c == '-' || c == 'L' || c == 'l')
        .filter(|s| !s.is_empty())
        .collect();

    if parts.len() >= 2 {
        let start = parts[0].parse::<usize>().unwrap_or(0);
        let end = parts[1].parse::<usize>().unwrap_or(start);
        (start, end)
    } else if let Ok(single) = clean.parse::<usize>() {
        (single, single)
    } else {
        (0, 0)
    }
}

fn calculate_severity(d: &Discrepancy) -> (usize, usize, usize, String) {
    let (c_start, c_end) = parse_line_span(&d.current_line_attr);
    let (e_start, e_end) = parse_line_span(&d.expected_line_attr);

    let line_drift = if c_start > 0 && e_start > 0 {
        c_start.abs_diff(e_start)
    } else {
        999
    };

    let c_span = if c_end >= c_start {
        (c_end - c_start).saturating_add(1)
    } else {
        1
    };
    let e_span = if e_end >= e_start {
        (e_end - e_start).saturating_add(1)
    } else {
        1
    };
    let span_drift = c_span.abs_diff(e_span);

    let mut score: usize = 0;
    let mut reasons = Vec::new();

    match &d.kind {
        DiscrepancyKind::MissingSourceTag { .. } => {
            score += 50_000;
            reasons.push("Missing Lua source link".to_string());
        }
        DiscrepancyKind::WrongFile { .. } => {
            score += 30_000;
            reasons.push("Points to wrong file".to_string());
        }
        DiscrepancyKind::RealmMismatch {
            wiki_realm,
            repo_realm,
        } => {
            score += 25_000;
            reasons.push(format!(
                "Incorrect realm (Wiki: {}, Code: {})",
                wiki_realm, repo_realm
            ));
        }
        DiscrepancyKind::MissingRealmTag { repo_realm } => {
            score += 22_000;
            reasons.push(format!("Missing realm tag (Code: {})", repo_realm));
        }
        DiscrepancyKind::FileNotFound { .. } => {
            score += 20_000;
            reasons.push("File not found (dead link)".to_string());
        }
        DiscrepancyKind::LineRangeDrift => {
            if c_start > 0 && c_end > 0 && c_start > c_end {
                score += 15_000;
                reasons.push("Inverted/backwards line range".to_string());
            } else if line_drift > 100 {
                reasons.push(format!("Extreme line drift ({} lines)", line_drift));
            } else if line_drift > 0 {
                reasons.push(format!("Shifted by {} lines", line_drift));
            }

            if line_drift > 0 {
                score = score.saturating_add(line_drift.saturating_mul(100));
            }

            if span_drift > 0 {
                score = score.saturating_add(span_drift.saturating_mul(10));
                if reasons.is_empty() {
                    reasons.push(format!("Range length changed (Δ{} lines)", span_drift));
                }
            }

            if d.current_line_attr.contains('-') && !d.current_line_attr.contains("-L") {
                score = score.saturating_add(50);
                if reasons.is_empty() {
                    reasons.push("Missing 'L' syntax".to_string());
                }
            }

            if reasons.is_empty() {
                reasons.push("Outdated line range".to_string());
            }
        }
    }

    (line_drift, span_drift, score, reasons.join(", "))
}

fn run_batch_check(
    analyzer: &mut Analyzer,
    snapshot: &git_repo::SourceSnapshot,
    pages: &[String],
    report_path: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut data = report::ReportData {
        snippets: Default::default(),
        quality: Vec::new(),
        page_views: Default::default(),
        panel_names: analyzer.panel_names()?,
        metrics: Vec::new(),
        findings: Vec::new(),
        source_issues: Vec::new(),
        missing_pages: Vec::new(),
        unresolved: Vec::new(),
        failed: Vec::new(),
    };
    let mut source_pages = 0;
    let mut canonical_source_pages = std::collections::HashSet::new();
    let mut alias_pages = 0;
    let mut no_lua = 0;
    let mut excluded = 0;
    let mut clean = 0;
    let mut source_issue_pages = 0;
    let mut source_kinds = [0usize; 6];
    for (idx, address) in pages.iter().enumerate() {
        print!("[{}/{}] Checking {}... ", idx + 1, pages.len(), address);
        match analyzer.check_page(address) {
            Ok(CheckPageOutcome::ValidSource { result }) => {
                if !canonical_source_pages.insert(result.page_address.clone()) {
                    alias_pages += 1;
                    println!(
                        "Alias of already checked source page {}",
                        result.page_address
                    );
                    continue;
                }
                source_pages += 1;
                let review =
                    result.findings.iter().any(|finding| {
                        finding.kind != audit_checks::FindingKind::ExampleOpportunity
                    }) || !result.unresolved_sources.is_empty();
                if result.discrepancies.is_empty() && !review {
                    clean += 1;
                    println!("OK");
                } else {
                    println!(
                        "{} source findings; {} additional findings{}",
                        result.discrepancies.len(),
                        result.findings.len(),
                        if result.unresolved_sources.is_empty() {
                            ""
                        } else {
                            "; unresolved source requires review"
                        }
                    );
                }
                data.findings.extend(result.findings);
                if !result.unresolved_sources.is_empty() {
                    data.unresolved
                        .push((result.page_address.clone(), result.unresolved_sources));
                }
                if !result.discrepancies.is_empty() {
                    source_issue_pages += 1;
                }
                for discrepancy in result.discrepancies {
                    let kind = match discrepancy.kind {
                        DiscrepancyKind::LineRangeDrift => 0,
                        DiscrepancyKind::MissingSourceTag { .. } => 1,
                        DiscrepancyKind::WrongFile { .. } => 2,
                        DiscrepancyKind::FileNotFound { .. } => 3,
                        DiscrepancyKind::RealmMismatch { .. } => 4,
                        DiscrepancyKind::MissingRealmTag { .. } => 5,
                    };
                    source_kinds[kind] += 1;
                    let (_, _, score, reason) = calculate_severity(&discrepancy);
                    let evidence = analyzer.discrepancy_evidence(&discrepancy);
                    data.source_issues.push(report::SourceIssue {
                        kind: discrepancy.kind,
                        page: discrepancy.page_address,
                        symbol: discrepancy.symbol_name,
                        current: discrepancy.current_line_attr,
                        expected: discrepancy.expected_line_attr,
                        score,
                        reason,
                        evidence,
                    });
                }
            }
            Ok(CheckPageOutcome::NoMatchingLuaSource { findings, .. }) => {
                no_lua += 1;
                println!(
                    "No matching Lua source; {} additional findings",
                    findings.len()
                );
                data.findings.extend(findings);
            }
            Ok(CheckPageOutcome::ExcludedSource { .. }) => {
                excluded += 1;
                println!("Skipped: intentionally excluded source or menu-only page");
            }
            Ok(CheckPageOutcome::NotFound) => {
                let evidence = analyzer.missing_page_evidence(address)?;
                data.missing_pages.push((address.clone(), evidence));
                println!("Missing page confirmed after alias/redirect checks");
            }
            Err(error) => {
                println!("Failed: {}", error);
                data.failed.push((address.clone(), error));
            }
        }
    }
    // check-list fetches its index lazily on the first page; capture views after
    // checking so every batch command uses the same fresh-index priority data.
    data.page_views = analyzer.wiki_client.page_views.clone();
    data.quality = analyzer.quality.values().cloned().collect();
    let review_pages = data
        .findings
        .iter()
        .filter(|finding| finding.kind != audit_checks::FindingKind::ExampleOpportunity)
        .map(|finding| &finding.page)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let mut metric = |name: &str, count, description: &str| {
        data.metrics
            .push((name.to_string(), count, description.to_string()))
    };
    metric(
        "Menu Source Files Excluded",
        analyzer.menu_files_excluded,
        "Menu directories and initialization files retained only as load evidence",
    );
    metric(
        "Menu-only Symbols Excluded",
        analyzer.menu_symbols_excluded,
        "Indexed definitions intentionally excluded from coverage and routine checks",
    );
    metric(
        "Metamethod Candidates Excluded",
        analyzer.metamethod_candidates_excluded,
        "Metamethod definitions do not require standalone documentation pages",
    );
    metric(
        "Total Pages Checked",
        pages.len(),
        "Existing wiki pages and repository-derived candidates evaluated",
    );
    metric(
        "Pages Analyzed for Lua Source",
        source_pages,
        "Pages with existing or discovered source definitions",
    );
    metric(
        "Source Page Aliases Reused",
        alias_pages,
        "Additional addresses resolving to an already checked canonical source page; findings counted once",
    );
    metric(
        "No Matching Lua Source",
        no_lua,
        "Native status unverified; page metadata and references are still checked",
    );
    metric(
        "Entirely Missing Pages",
        data.missing_pages.len(),
        "Absent after official-index, alias, redirect, and live-page checks",
    );
    metric(
        "Pages with Source Discrepancies",
        source_issue_pages,
        "Pages with source-range, file, or realm findings",
    );
    metric(
        "Pages with Additional Findings",
        review_pages,
        "Unique pages with structural, identity, contract, reference, or load-path findings",
    );
    metric(
        "Clean Source Pages",
        clean,
        "No source or additional findings detected within supported checks",
    );
    metric(
        "Failed Checks",
        data.failed.len(),
        "Uncompleted page checks; never counted as missing or clean",
    );
    metric(
        "Excluded Source Pages",
        excluded,
        "Sandbox, TTT, and menu-only source pages skipped",
    );
    metric(
        "Unresolved Source Pages",
        data.unresolved.len(),
        "Source definitions requiring manual investigation",
    );
    for (idx, name) in [
        "Line Range Issues",
        "Missing Source Tags",
        "Wrong File Paths",
        "Dead Source Links",
        "Realm Mismatches",
        "Missing Realm Tags",
    ]
    .into_iter()
    .enumerate()
    {
        metric(name, source_kinds[idx], "Individual source/realm findings");
    }
    metric(
        "Signature Comparisons",
        analyzer.signature_comparisons,
        "Function blocks with known Lua signatures, accounting for source variants",
    );
    metric(
        "Return Comparisons",
        analyzer.return_comparisons,
        "Function blocks with statically bounded return shapes",
    );
    metric(
        "Load Path Realm Comparisons",
        analyzer.load_comparisons,
        "Function blocks with positively traced engine/include paths",
    );
    metric(
        "Traced Source Files",
        analyzer.load_graph.traced_files(),
        "Source files reachable through modeled engine roots and literal load operations",
    );
    metric(
        "Unresolved Load Operations",
        analyzer.load_graph.unresolved.len(),
        "Dynamic, conditional, deferred, or missing literal load paths",
    );
    metric(
        "Reference Verification Requests",
        analyzer.wiki_client.reference_requests,
        "Fresh requests for unindexed targets and missing-page candidates; results reused",
    );
    metric(
        "Realm Comparisons",
        analyzer.realm_checked,
        "Existing direct source-realm comparisons",
    );
    metric(
        "Inconclusive Realm Checks",
        analyzer.realm_unresolved,
        "Direct realm checks lacking sufficient evidence",
    );
    println!("\nAudit Summary");
    for (name, count, _) in &data.metrics {
        println!("{}: {}", name, count);
    }
    for kind in audit_checks::FindingKind::ALL {
        println!(
            "{}: {}",
            kind.label(),
            data.findings
                .iter()
                .filter(|finding| finding.kind == kind)
                .count()
        );
    }
    if let Some(path) = report_path {
        for evidence in data
            .findings
            .iter()
            .flat_map(|f| f.sources.iter())
            .chain(
                data.source_issues
                    .iter()
                    .filter_map(|f| f.evidence.as_ref()),
            )
            .chain(data.missing_pages.iter().filter_map(|(_, e)| e.as_ref()))
        {
            if let Some(excerpt) = analyzer.report_excerpt(evidence) {
                data.snippets.insert(
                    snapshot.source_url(&evidence.file, evidence.start, evidence.end),
                    excerpt,
                );
            }
        }
        fs::write(path, report::render(snapshot, &data))?;
        println!("Generated audit tables and evidence: {}", path.display());
    }
    if !data.failed.is_empty() {
        return Err(format!(
            "Audit incomplete: {} page checks failed; partial findings are retained in the report",
            data.failed.len()
        )
        .into());
    }
    Ok(())
}
