//! One policy owns documentation scoring for the analyzer and report. This is
//! observable coverage, not semantic correctness. Revisit these rules if manual
//! ratings show that a criterion systematically penalizes sufficient short docs.
use crate::wiki_markup::{self, Block};

#[derive(Clone, Debug)]
pub struct Criterion {
    pub name: &'static str,
    pub earned: usize,
    pub possible: usize,
    pub explanation: &'static str,
}

#[derive(Clone, Debug)]
pub struct Rating {
    pub page: String,
    pub score: usize,
    pub criteria: Vec<Criterion>,
    pub sources: Vec<String>,
    pub critical: bool,
}

fn text(markup: &str) -> String {
    let mut out = String::new();
    let mut offset = 0;
    for tag in wiki_markup::tags(markup) {
        out.push_str(&markup[offset..tag.start]);
        out.push(' ');
        offset = tag.end;
    }
    out.push_str(&markup[offset..]);
    wiki_markup::decode(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn useful(markup: &str) -> bool {
    let clean = text(markup).to_lowercase();
    let clean = clean.trim_matches(|c: char| !c.is_alphanumeric());
    !clean.is_empty()
        && !matches!(
            clean,
            "todo" | "tbd" | "wip" | "undocumented" | "no description" | "description needed"
        )
}

fn needs_detail(block: &Block) -> bool {
    let kind = block
        .tag
        .attrs
        .get("type")
        .map(String::as_str)
        .unwrap_or("");
    kind.contains("table")
        || kind.contains("function")
        || block.tag.attrs.get("name").is_some_and(|name| {
            matches!(
                name.to_lowercase().as_str(),
                "data" | "options" | "flags" | "mode" | "value" | "callback" | "func" | "..."
            )
        })
}

fn detail_present(block: &Block) -> bool {
    useful(&block.inner)
        || !wiki_markup::blocks(&block.inner, "args").is_empty()
        || block
            .tag
            .attrs
            .get("default")
            .is_some_and(|default| !default.trim().is_empty())
}

pub fn rate(page: &str, markup: &str) -> Option<Rating> {
    let functions = wiki_markup::outer_blocks(markup, "function");
    if functions.is_empty() {
        return None;
    }
    let mut criteria = Vec::new();
    let mut total_earned = 0;
    let mut total_possible = 0;
    let mut add = |name, weight, covered: usize, expected: usize, explanation| {
        if expected == 0 {
            return;
        }
        let earned = (weight * covered + expected / 2) / expected;
        criteria.push(Criterion {
            name,
            earned,
            possible: weight,
            explanation,
        });
        total_earned += earned;
        total_possible += weight;
    };
    let descriptions: Vec<_> = functions
        .iter()
        .map(|f| wiki_markup::blocks(&f.inner, "description"))
        .collect();
    let purposes = descriptions
        .iter()
        .filter(|blocks| blocks.iter().any(|b| useful(&b.inner)))
        .count();
    add(
        "Purpose",
        40,
        purposes,
        functions.len(),
        "A nonempty, non-placeholder description for each declared function. No minimum word count.",
    );
    let args: Vec<_> = functions
        .iter()
        .flat_map(|f| {
            wiki_markup::blocks(&f.inner, "args")
                .into_iter()
                .take(1)
                .flat_map(|b| wiki_markup::outer_blocks(&b.inner, "arg"))
        })
        .collect();
    let returns: Vec<_> = functions
        .iter()
        .flat_map(|f| {
            wiki_markup::blocks(&f.inner, "rets")
                .into_iter()
                .take(1)
                .flat_map(|b| wiki_markup::outer_blocks(&b.inner, "ret"))
        })
        .collect();
    let typed = |blocks: &[Block]| {
        blocks
            .iter()
            .filter(|b| {
                b.tag
                    .attrs
                    .get("type")
                    .is_some_and(|t| !t.trim().is_empty())
            })
            .count()
    };
    add(
        "Argument types",
        20,
        typed(&args),
        args.len(),
        "Declared arguments have explicit types. No penalty when there are no declared arguments.",
    );
    let complex_args: Vec<_> = args.iter().filter(|b| needs_detail(b)).collect();
    add(
        "Argument guidance",
        10,
        complex_args.iter().filter(|b| detail_present(b)).count(),
        complex_args.len(),
        "Details/defaults or a nested callback contract for table/function types and names data, options, flags, mode, value, callback, func or ...; simple named inputs are exempt.",
    );
    add(
        "Return types",
        15,
        typed(&returns),
        returns.len(),
        "Declared returns have explicit types. Absent returns are not assumed to mean zero returns and are not penalized.",
    );
    let callback = functions.iter().any(crate::audit_checks::callback_contract);
    let complex_returns: Vec<_> = returns
        .iter()
        .filter(|b| {
            needs_detail(b) || (callback && b.tag.attrs.get("type").is_some_and(|t| t == "boolean"))
        })
        .collect();
    add(
        "Return guidance",
        10,
        complex_returns.iter().filter(|b| detail_present(b)).count(),
        complex_returns.len(),
        "Complex returns and hook/event booleans explain their meaning. Hook return behavior is not inferred from the default implementation.",
    );
    add(
        "Realm",
        5,
        functions
            .iter()
            .filter(|f| {
                wiki_markup::blocks(&f.inner, "realm")
                    .iter()
                    .any(|b| useful(&b.inner))
            })
            .count(),
        functions.len(),
        "Each function declares its realm; this checks presence, not correctness.",
    );
    let example_expected = !callback
        && (args.len() >= 3
            || args.iter().any(|b| {
                b.tag
                    .attrs
                    .get("type")
                    .is_some_and(|t| t.contains("table") || t.contains("function"))
            }));
    let has_example = wiki_markup::blocks(markup, "example")
        .iter()
        .any(|b| useful(&b.inner));
    add(
        "Example support",
        5,
        usize::from(has_example),
        usize::from(example_expected),
        "An example is a small optional signal only for calls with at least three arguments or table/function inputs. Simple APIs and hooks are exempt.",
    );
    let score = (100 * total_earned + total_possible / 2) / total_possible;
    // Only an observably empty description and no usable argument/return/example
    // guidance can enter Fixes. A low heuristic score alone never does.
    let critical = score <= 20
        && purposes == 0
        && !has_example
        && !args.iter().chain(&returns).any(|b| useful(&b.inner));
    Some(Rating {
        page: page.into(),
        score,
        criteria,
        critical,
        sources: wiki_markup::blocks(markup, "file")
            .iter()
            .map(|b| b.inner.trim().to_string())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_sufficient_docs_score_as_well_as_long_docs() {
        let short = "<function name='GetMap'><description>Returns the current map.</description><realm>Shared</realm><rets><ret type='string'/></rets></function>";
        assert_eq!(rate("game.GetMap", short).unwrap().score, 100);
        assert_eq!(
            rate(
                "game.GetMap",
                &short.replace(
                    "Returns the current map.",
                    "Returns the current map. More explanatory prose does not add points."
                )
            )
            .unwrap()
            .score,
            100
        );
        assert!(rate("Guide", "plain guide text").is_none());
    }
    #[test]
    fn applicability_and_empty_docs_are_explicit() {
        let minimal = rate(
            "Empty",
            "<function name='Empty'><description>TODO</description></function>",
        )
        .unwrap();
        assert_eq!(minimal.score, 0);
        assert!(minimal.critical);
        let hook = rate("GM:Callback", "<function name='Callback' type='hook'><description>Runs during rendering.</description><realm>Client</realm><rets><ret type='boolean'>Return true to stop drawing.</ret></rets></function>").unwrap();
        assert_eq!(hook.score, 100);
        assert!(!hook.critical);
        let complex = rate("lib.Call", "<function name='Call'><description>Registers a callback.</description><realm>Shared</realm><args><arg type='function' name='callback'/></args></function>").unwrap();
        assert!(complex.score < 100 && !complex.critical);
        assert_eq!(
            complex
                .criteria
                .iter()
                .find(|c| c.name == "Argument guidance")
                .unwrap()
                .earned,
            0
        );
    }
}
