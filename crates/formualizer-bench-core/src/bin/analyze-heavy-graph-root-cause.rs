use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;

const WORKBOOK: &str = "Fossil_EstimatingTemplate_2026-08_21_A.xlsx";
const EDGE_DUMP: &str = r"docs/issue-solutions/data/heavy-scc-edge-dump.tsv";
const STATIC_EDGE_DUMP: &str = r"docs/issue-solutions/data/heavy-static-scc-edge-dump.tsv";
const MISMATCHES: &str =
    r"docs/issue-solutions/data/heavy-formualizer-excel-mismatch-inventory.json";
const OUTPUT: &str = r"docs/issue-solutions/data/heavy-graph-root-cause.json";

const STATIC_FAMILIES: [&str; 8] = [
    "direct_cell",
    "range",
    "whole_row",
    "whole_column",
    "named_range",
    "table",
    "dynamic_reference",
    "other",
];
const RUNTIME_FAMILIES: [&str; 8] = [
    "direct_cell",
    "range",
    "whole_row",
    "whole_column",
    "named_range",
    "table",
    "dynamic_reference",
    "other",
];

#[derive(Clone)]
struct Member {
    member_index: usize,
    address: String,
    normalized: String,
    sheet: Option<String>,
    dynamic: bool,
    volatile: bool,
    formula_debug: String,
}

#[derive(Clone, Copy)]
struct Edge {
    source: usize,
    target: usize,
    mask: u16,
}

fn normalize_address(address: &str) -> String {
    if let Some((sheet, cell)) = address.split_once('!') {
        format!(
            "{}!{}",
            sheet
                .trim_matches('\'')
                .replace("''", "'")
                .to_ascii_uppercase(),
            cell.replace('$', "").to_ascii_uppercase()
        )
    } else {
        address.replace('$', "").to_ascii_uppercase()
    }
}

fn sheet_of(address: &str) -> Option<String> {
    address
        .split_once('!')
        .map(|(sheet, _)| sheet.to_ascii_uppercase())
}

fn first_pass_order(adjacency: &[Vec<usize>]) -> Vec<usize> {
    fn visit(node: usize, adjacency: &[Vec<usize>], seen: &mut [bool], order: &mut Vec<usize>) {
        if seen[node] {
            return;
        }
        seen[node] = true;
        for &target in &adjacency[node] {
            if !seen[target] {
                visit(target, adjacency, seen, order);
            }
        }
        order.push(node);
    }
    let mut seen = vec![false; adjacency.len()];
    let mut order = Vec::with_capacity(adjacency.len());
    for node in 0..adjacency.len() {
        visit(node, adjacency, &mut seen, &mut order);
    }
    order
}

fn components<F>(node_count: usize, edges: &[Edge], keep: F) -> (Vec<Vec<usize>>, usize)
where
    F: Fn(Edge) -> bool,
{
    let mut adjacency = vec![Vec::new(); node_count];
    let mut reverse = vec![Vec::new(); node_count];
    let mut kept = 0;
    for &edge in edges {
        if keep(edge) {
            adjacency[edge.source].push(edge.target);
            reverse[edge.target].push(edge.source);
            kept += 1;
        }
    }
    let order = first_pass_order(&adjacency);
    let mut seen = vec![false; node_count];
    let mut out = Vec::new();
    for &root in order.iter().rev() {
        if seen[root] {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![root];
        seen[root] = true;
        while let Some(node) = stack.pop() {
            component.push(node);
            for &target in &reverse[node] {
                if !seen[target] {
                    seen[target] = true;
                    stack.push(target);
                }
            }
        }
        component.sort_unstable();
        out.push(component);
    }
    (out, kept)
}

fn path_between<F>(node_count: usize, edges: &[Edge], start: usize, goal: F) -> Option<Vec<usize>>
where
    F: Fn(usize) -> bool,
{
    let mut adjacency = vec![Vec::new(); node_count];
    for edge in edges {
        adjacency[edge.source].push(edge.target);
    }
    let mut queue = VecDeque::from([start]);
    let mut parent = vec![None; node_count];
    let mut seen = vec![false; node_count];
    seen[start] = true;
    let mut end = None;
    while let Some(node) = queue.pop_front() {
        if node != start && goal(node) {
            end = Some(node);
            break;
        }
        for &target in &adjacency[node] {
            if !seen[target] {
                seen[target] = true;
                parent[target] = Some(node);
                queue.push_back(target);
            }
        }
    }
    let mut node = end?;
    let mut path = vec![node];
    while let Some(previous) = parent[node] {
        path.push(previous);
        node = previous;
    }
    path.reverse();
    Some(path)
}

fn metrics(
    members: &[Member],
    edges: &[Edge],
    original_static: &HashSet<String>,
    mismatches: &HashSet<String>,
) -> Value {
    let (components, edge_count) = components(members.len(), edges, |_| true);
    let cyclic: Vec<&Vec<usize>> = components
        .iter()
        .filter(|component| {
            component.len() > 1
                || edges
                    .iter()
                    .any(|edge| edge.source == edge.target && component.contains(&edge.source))
        })
        .collect();
    let largest = components
        .iter()
        .max_by_key(|component| component.len())
        .cloned()
        .unwrap_or_default();
    let largest_addresses: HashSet<String> = largest
        .iter()
        .map(|&index| members[index].normalized.clone())
        .collect();
    let mut inputs = 0;
    let mut engine = 0;
    for &index in &largest {
        match members[index].sheet.as_deref() {
            Some("CASHFLOW INPUTS") => inputs += 1,
            Some("CASHFLOW ENGINE") => engine += 1,
            _ => {}
        }
    }
    let cross_sheet_cycle = cyclic.iter().any(|component| {
        let sheets: HashSet<&str> = component
            .iter()
            .filter_map(|&index| members[index].sheet.as_deref())
            .collect();
        sheets.len() > 1
    });
    json!({
        "largest_scc": largest.len(),
        "cyclic_scc_count": cyclic.len(),
        "original_static_members_retained": largest_addresses.intersection(original_static).count(),
        "main_component_mismatch_members": largest_addresses.intersection(mismatches).count(),
        "cashflow_inputs_members": inputs,
        "cashflow_engine_members": engine,
        "cross_sheet_cycle": cross_sheet_cycle,
        "edge_count": edge_count,
    })
}

fn witnesses(members: &[Member], edges: &[Edge]) -> Vec<Value> {
    let inputs: Vec<usize> = members
        .iter()
        .enumerate()
        .filter_map(|(i, m)| (m.sheet.as_deref() == Some("CASHFLOW INPUTS")).then_some(i))
        .collect();
    let engine: Vec<usize> = members
        .iter()
        .enumerate()
        .filter_map(|(i, m)| (m.sheet.as_deref() == Some("CASHFLOW ENGINE")).then_some(i))
        .collect();
    let mut out = Vec::new();
    for (sources, targets, direction) in [
        (&inputs, &engine, "CashFlow Inputs -> CashFlow Engine"),
        (&engine, &inputs, "CashFlow Engine -> CashFlow Inputs"),
    ] {
        let target_set: HashSet<usize> = targets.iter().copied().collect();
        for &source in sources {
            if let Some(path) = path_between(members.len(), edges, source, |node| {
                target_set.contains(&node)
            }) {
                out.push(json!({
                    "direction": direction,
                    "member_indices": path,
                    "addresses": path.iter().map(|&i| members[i].address.clone()).collect::<Vec<_>>(),
                    "formulas": path.iter().map(|&i| if members[i].formula_debug.is_empty() { Value::Null } else { Value::String(members[i].formula_debug.clone()) }).collect::<Vec<_>>(),
                }));
                break;
            }
        }
    }
    out
}

fn variants(
    prefix: &str,
    base: &[Edge],
    family_count: usize,
    families: &[&str],
    members: &[Member],
    original_static: &HashSet<String>,
    mismatches: &HashSet<String>,
    mismatch_indices: &HashSet<usize>,
    unsupported_indices: &HashSet<usize>,
    numeric_error_indices: &HashSet<usize>,
    conditional_indices: &HashSet<usize>,
) -> Vec<Value> {
    let mut out = Vec::new();
    let add =
        |out: &mut Vec<Value>, name: String, base: &[Edge], predicate: &dyn Fn(Edge) -> bool| {
            let kept: Vec<Edge> = base
                .iter()
                .copied()
                .filter(|edge| predicate(*edge))
                .collect();
            let mut result = metrics(members, &kept, original_static, mismatches);
            if let Value::Object(ref mut object) = result {
                object.insert("variant".into(), Value::String(name));
                object.insert("removed_edges".into(), Value::from(base.len() - kept.len()));
                object.insert("witnesses".into(), Value::Array(witnesses(members, &kept)));
            }
            out.push(result);
        };
    add(&mut out, format!("{prefix}_all"), base, &|_| true);
    add(
        &mut out,
        format!("{prefix}_direct_exact_cell_only"),
        base,
        &|edge| edge.mask == 1,
    );
    for (index, family) in families.iter().take(family_count).enumerate() {
        let bit = 1u16 << index;
        add(
            &mut out,
            format!("{prefix}_without_{family}_origin"),
            base,
            &|edge| edge.mask & bit == 0,
        );
    }
    add(
        &mut out,
        format!("{prefix}_without_known_mismatch_source_edges"),
        base,
        &|edge| !mismatch_indices.contains(&edge.source),
    );
    add(
        &mut out,
        format!("{prefix}_without_unsupported_or_name_source_edges"),
        base,
        &|edge| !unsupported_indices.contains(&edge.source),
    );
    add(
        &mut out,
        format!("{prefix}_without_excel_numeric_to_error_source_edges"),
        base,
        &|edge| !numeric_error_indices.contains(&edge.source),
    );
    add(
        &mut out,
        format!("{prefix}_without_conditional_mismatch_source_edges"),
        base,
        &|edge| !conditional_indices.contains(&edge.source),
    );
    if let Some(index) = families.iter().position(|family| *family == "named_range") {
        let bit = 1u16 << index;
        add(
            &mut out,
            format!("{prefix}_mismatch_sources_plus_named_origins"),
            base,
            &|edge| !mismatch_indices.contains(&edge.source) && edge.mask & bit == 0,
        );
    }
    if let Some(index) = families.iter().position(|family| *family == "range") {
        let bit = 1u16 << index;
        add(
            &mut out,
            format!("{prefix}_unsupported_sources_plus_range_origins"),
            base,
            &|edge| !unsupported_indices.contains(&edge.source) && edge.mask & bit == 0,
        );
    }
    if let Some(index) = families
        .iter()
        .position(|family| *family == "dynamic_reference")
    {
        let bit = 1u16 << index;
        add(
            &mut out,
            format!("{prefix}_conditional_mismatch_plus_dynamic_origins"),
            base,
            &|edge| !conditional_indices.contains(&edge.source) && edge.mask & bit == 0,
        );
    }
    out
}

fn main() -> Result<()> {
    let baseline: Value = serde_json::from_str(&fs::read_to_string(
        r"docs/issue-solutions/data/latest-upstream-heavy-baseline.json",
    )?)?;
    let runtime_topology: Value = serde_json::from_str(&fs::read_to_string(
        r"docs/issue-solutions/data/fossil-runtime-live-scc-topology.json",
    )?)?;
    let static_origins: Value = serde_json::from_str(&fs::read_to_string(
        r"docs/issue-solutions/data/fossil-static-edge-origin-breakdown.json",
    )?)?;
    let mismatch_inventory: Value = serde_json::from_str(&fs::read_to_string(MISMATCHES)?)?;
    let original_static: HashSet<String> =
        baseline["steps"][0]["main_passes"][0]["changed_member_addresses"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(Value::as_str)
            .filter(|address| address.contains('!'))
            .map(normalize_address)
            .collect();
    let mismatch_items = mismatch_inventory["main_scc_mismatches"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mismatches: HashSet<String> = mismatch_items
        .iter()
        .filter_map(|item| item["address"].as_str())
        .map(normalize_address)
        .collect();
    let unsupported: HashSet<String> = mismatch_items
        .iter()
        .filter(|item| {
            item["formula_features"].as_array().is_some_and(|features| {
                features
                    .iter()
                    .any(|f| f.as_str() == Some("unsupported_or_xlfn"))
            }) || item["category"].as_str() == Some("excel_numeric_formualizer_nimpl_error")
                || item["formualizer"]["error_kind"]
                    .as_str()
                    .is_some_and(|kind| kind == "Name" || kind == "NImpl")
        })
        .filter_map(|item| item["address"].as_str())
        .map(normalize_address)
        .collect();
    let numeric_errors: HashSet<String> = mismatch_items
        .iter()
        .filter(|item| {
            item["category"]
                .as_str()
                .is_some_and(|category| category.starts_with("excel_numeric_formualizer_"))
        })
        .filter_map(|item| item["address"].as_str())
        .map(normalize_address)
        .collect();
    let conditional: HashSet<String> = mismatch_items
        .iter()
        .filter(|item| {
            item["formula_features"]
                .as_array()
                .is_some_and(|features| features.iter().any(|f| f.as_str() == Some("conditional")))
        })
        .filter_map(|item| item["address"].as_str())
        .map(normalize_address)
        .collect();

    let dump = fs::read_to_string(EDGE_DUMP).with_context(|| format!("read {EDGE_DUMP}"))?;
    let mut members = Vec::new();
    let mut static_pairs: Vec<(usize, usize, u16)> = Vec::new();
    let mut runtime_edges = Vec::new();
    for line in dump.lines().skip(1) {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.first().copied() {
            Some("M") if fields.len() >= 7 => members.push(Member {
                member_index: fields[1].parse()?,
                address: fields[3].into(),
                normalized: normalize_address(fields[3]),
                sheet: sheet_of(fields[3]),
                dynamic: fields[4] == "true",
                volatile: fields[5] == "true",
                formula_debug: fields[6].into(),
            }),
            Some("R") if fields.len() >= 4 => runtime_edges.push(Edge {
                source: fields[1].parse()?,
                target: fields[2].parse()?,
                mask: fields[3].parse()?,
            }),
            _ => {}
        }
    }
    let static_dump =
        fs::read_to_string(STATIC_EDGE_DUMP).with_context(|| format!("read {STATIC_EDGE_DUMP}"))?;
    for line in static_dump.lines().skip(1) {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.first().copied() == Some("S") && fields.len() >= 3 {
            let mask = fields
                .get(3)
                .and_then(|field| field.parse().ok())
                .unwrap_or(1);
            static_pairs.push((fields[1].parse()?, fields[2].parse()?, mask));
        }
    }
    members.sort_by_key(|member| member.member_index);
    if members.is_empty() {
        anyhow::bail!("edge dump has no members")
    }
    let mut static_edges = Vec::new();
    let mut seen_static_edges = HashSet::new();
    for (source, target, mask) in static_pairs {
        if seen_static_edges.insert((source, target)) {
            static_edges.push(Edge {
                source,
                target,
                mask,
            });
        } else if let Some(edge) = static_edges
            .iter_mut()
            .find(|edge| edge.source == source && edge.target == target)
        {
            edge.mask |= mask;
        }
    }
    let address_to_index: std::collections::HashMap<String, usize> = members
        .iter()
        .enumerate()
        .map(|(i, member)| (member.normalized.clone(), i))
        .collect();
    let mismatch_indices: HashSet<usize> = mismatches
        .iter()
        .filter_map(|address| address_to_index.get(address).copied())
        .collect();
    let unsupported_indices: HashSet<usize> = unsupported
        .iter()
        .filter_map(|address| address_to_index.get(address).copied())
        .collect();
    let numeric_error_indices: HashSet<usize> = numeric_errors
        .iter()
        .filter_map(|address| address_to_index.get(address).copied())
        .collect();
    let conditional_indices: HashSet<usize> = conditional
        .iter()
        .filter_map(|address| address_to_index.get(address).copied())
        .collect();
    let static_variants = variants(
        "static_graph",
        &static_edges,
        STATIC_FAMILIES.len(),
        &STATIC_FAMILIES,
        &members,
        &original_static,
        &mismatches,
        &mismatch_indices,
        &unsupported_indices,
        &numeric_error_indices,
        &conditional_indices,
    );
    let runtime_variants = variants(
        "runtime_observed",
        &runtime_edges,
        RUNTIME_FAMILIES.len(),
        &RUNTIME_FAMILIES,
        &members,
        &original_static,
        &mismatches,
        &mismatch_indices,
        &unsupported_indices,
        &numeric_error_indices,
        &conditional_indices,
    );
    let static_base = metrics(&members, &static_edges, &original_static, &mismatches);
    let runtime_base = metrics(&members, &runtime_edges, &original_static, &mismatches);
    let static_dump_origin_counts: HashMap<String, usize> = STATIC_FAMILIES
        .iter()
        .enumerate()
        .map(|(index, family)| {
            (
                family.to_string(),
                static_edges
                    .iter()
                    .filter(|edge| edge.mask & (1 << index) != 0)
                    .count(),
            )
        })
        .collect();
    let runtime_dump_origin_counts: HashMap<String, usize> = RUNTIME_FAMILIES
        .iter()
        .enumerate()
        .map(|(index, family)| {
            (
                family.to_string(),
                runtime_edges
                    .iter()
                    .filter(|edge| edge.mask & (1 << index) != 0)
                    .count(),
            )
        })
        .collect();
    let mut member_summary = Vec::new();
    for member in &members {
        member_summary.push(json!({"address": member.address, "normalized_address": member.normalized, "dynamic": member.dynamic, "volatile": member.volatile, "formula_debug": if member.formula_debug.is_empty() { Value::Null } else { Value::String(member.formula_debug.clone()) }}));
    }
    let result = json!({
        "schema": "formualizer.heavy-graph-root-cause/v3",
        "workbook": WORKBOOK,
        "input": "Inputs!F7=300",
        "baseline": {"static_graph": static_base, "runtime_observed_graph": runtime_base, "prior_static_scc_artifact": {"largest_scc": baseline["static_scc_probe"]["largest_scc_size"], "cyclic_scc_count": baseline["static_scc_probe"]["cyclic_scc_count"], "runtime_live_members": runtime_topology["runtime_live_cycle_member_count"], "static_origin_counts": static_origins["origin_counts"]}},
        "edge_taxonomy": {"static_labels": STATIC_FAMILIES, "runtime_labels": RUNTIME_FAMILIES, "static_dump_origin_counts": static_dump_origin_counts, "runtime_observed_origin_counts": runtime_dump_origin_counts},
        "main_scc_internal_edge_counts": {"static_internal_edge_count": static_edges.len(), "runtime_observed_internal_edge_count": runtime_edges.len(), "prior_static_internal_live_edge_count": static_origins["static_internal_live_edge_count"], "prior_static_origin_counts": static_origins["origin_counts"]},
        "cycle_witnesses": {"static_graph": witnesses(&members, &static_edges), "runtime_observed_graph": witnesses(&members, &runtime_edges)},
        "semantic_mismatches_in_main_scc": {"count": mismatch_items.len(), "category_counts": mismatch_inventory["main_scc_mismatch_counts"], "items": mismatch_items, "runtime_live_membership": "unknown except explicit prior samples"},
        "mismatch_ablations": static_variants.iter().chain(runtime_variants.iter()).filter(|variant| variant["variant"].as_str().is_some_and(|name| name.contains("mismatch") || name.contains("unsupported"))).cloned().collect::<Vec<_>>(),
        "dependency_family_ablations": static_variants.iter().chain(runtime_variants.iter()).filter(|variant| variant["variant"].as_str().is_some_and(|name| name.contains("without_") || name.contains("direct_exact"))).cloned().collect::<Vec<_>>(),
        "cross_ablations": static_variants.iter().chain(runtime_variants.iter()).filter(|variant| variant["variant"].as_str().is_some_and(|name| name.contains('+'))).cloned().collect::<Vec<_>>(),
        "graph_variants": {"static_graph": static_variants, "runtime_observed_graph": runtime_variants},
        "remaining_feedback_backbone": {"static": witnesses(&members, &static_edges), "runtime_observed": witnesses(&members, &runtime_edges)},
        "excel_assisted_reference_evidence": {"status": "none available", "reason": "Excel returned zero Heavy circular seeds and no trace paths."},
        "excel_assisted_graph": {"status": "not constructed", "uncertain_references_retained": true},
        "targeted_semantic_corrections": {"status": "none", "reason": "No isolated Level C correction was justified."},
        "root_cause_classification": {"primary": "UNRESOLVED_INTERACTION_LIKELY", "best_supported_hypothesis": "H3/H2 over H1", "confidence": "medium", "reason": "Excel exposes no Heavy circular seed/path; Formualizer graph and semantic mismatch evidence are both substantial, but no isolated correction proves causality."},
        "engine_v2_implications": {"recommendation": ["precise Excel-compatible formula/reference semantics", "demand-driven runtime reference discovery", "runtime cycle discovery after actual feedback", "retained workspace only as a later proven-safe optimization"], "static_scc_primary": false},
        "members": member_summary,
        "raw_edge_dump": {"runtime": EDGE_DUMP, "static": STATIC_EDGE_DUMP},
        "notes": ["Graph ablations establish graph causality only; removal does not establish incorrectness.", "Static edges come from the evaluator graph dump; runtime edges come from final SCC live edges.", "Excel evidence remains primary for circularity claims."]
    });
    let output_path = PathBuf::from(OUTPUT);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output_path, serde_json::to_string_pretty(&result)? + "\n")?;
    println!("Generated {}", output_path.display());
    Ok(())
}
