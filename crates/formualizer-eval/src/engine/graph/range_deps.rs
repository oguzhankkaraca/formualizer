use super::*;
use crate::engine::used_extent::{ExtentPolicy, OpenRangeBounds, resolve_used_extent};
use formualizer_common::LiteralValue;
use formualizer_parse::parser::{ASTNode, ASTNodeType, ReferenceType};

#[derive(Clone, Copy, PartialEq, Eq)]
enum RangeSelfUse {
    NoMatch,
    Excluded,
    IncludedOrUnknown,
}

impl RangeSelfUse {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::IncludedOrUnknown, _) | (_, Self::IncludedOrUnknown) => Self::IncludedOrUnknown,
            (Self::Excluded, _) | (_, Self::Excluded) => Self::Excluded,
            _ => Self::NoMatch,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum StructuralEdit {
    InsertRows { before: u32 },
    DeleteRows { start: u32, end: u32 },
    InsertColumns { before: u32 },
    DeleteColumns { start: u32, end: u32 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompactDependencyPrototypeStats {
    pub expanded_graph_edges: usize,
    pub formula_vertices: usize,
    pub range_dependent_formula_count: usize,
    pub symbolic_range_record_count: usize,
    pub stripe_membership_record_count: usize,
    pub named_dependency_record_count: usize,
    pub table_dependency_record_count: usize,
    pub conditional_formula_count: usize,
    pub spill_anchor_count: usize,
    pub cross_sheet_dependency_record_count: usize,
    pub dynamic_dependency_descriptor_count: usize,
    pub compact_record_count: usize,
    pub estimated_compact_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompactSymbolicSccStats {
    pub vertex_count: usize,
    pub direct_edge_count: usize,
    pub range_descriptor_count: usize,
    pub range_neighbor_visits: usize,
    pub transient_expanded_edge_count: usize,
    pub max_logical_out_degree: usize,
    pub scc_count: usize,
    pub cyclic_scc_count: usize,
    pub largest_scc_size: usize,
    pub largest_cyclic_scc_size: usize,
    pub scc_partition_fingerprint: u64,
    pub transient_bytes_estimate: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompactDependencyPrototypeValidation {
    pub expanded_formula_edges: usize,
    pub direct_cell_edges: usize,
    pub symbolic_range_edges: usize,
    pub named_edges: usize,
    pub table_edges: usize,
    pub unclassified_edges: usize,
    pub unclassified_kind_counts: Vec<(String, usize)>,
    pub unclassified_samples: Vec<(u32, u32, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompactDirtySetParity {
    pub compact_count: usize,
    pub oracle_count: usize,
    pub missing_from_compact: usize,
    pub extra_in_compact: usize,
    pub exact: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StructuralOccupancy {
    occupied_rows: Vec<u32>,
    occupied_columns: Vec<u32>,
    conservative: bool,
}

impl StructuralOccupancy {
    pub(crate) fn conservative() -> Self {
        Self {
            conservative: true,
            ..Self::default()
        }
    }

    fn finish(&mut self) {
        self.occupied_rows.sort_unstable();
        self.occupied_rows.dedup();
        self.occupied_columns.sort_unstable();
        self.occupied_columns.dedup();
    }

    pub(crate) fn include_arrow_sheet(&mut self, sheet: &crate::arrow_store::ArrowSheet) {
        let shapes = sheet.shape();
        for (col, column) in sheet.columns.iter().enumerate() {
            let shape_occupied = shapes.get(col).is_some_and(|shape| {
                shape.has_num || shape.has_bool || shape.has_text || shape.has_err
            });
            let sparse_meta_occupied = column.sparse_chunks.values().any(|chunk| {
                chunk.meta.non_null_num > 0
                    || chunk.meta.non_null_bool > 0
                    || chunk.meta.non_null_text > 0
                    || chunk.meta.non_null_err > 0
            });
            let overlay_occupied = column
                .chunks
                .iter()
                .chain(column.sparse_chunks.values())
                .any(|chunk| {
                    chunk.overlay.iter().next().is_some()
                        || chunk.computed_overlay.iter().next().is_some()
                });
            if shape_occupied || sparse_meta_occupied || overlay_occupied {
                self.occupied_columns.push(col as u32);
            }
        }
        self.finish();
    }

    fn intersects(sorted: &[u32], start: u32, end: u32) -> bool {
        let index = sorted.partition_point(|value| *value < start);
        sorted.get(index).is_some_and(|value| *value <= end)
    }

    fn cross_axis_occupied(self_ref: &Self, edit: StructuralEdit, start: u32, end: u32) -> bool {
        if self_ref.conservative {
            return true;
        }
        match edit {
            StructuralEdit::InsertRows { .. } | StructuralEdit::DeleteRows { .. } => {
                Self::intersects(&self_ref.occupied_columns, start, end)
            }
            StructuralEdit::InsertColumns { .. } | StructuralEdit::DeleteColumns { .. } => {
                Self::intersects(&self_ref.occupied_rows, start, end)
            }
        }
    }
}

fn ast_contains_conditional(node: &ASTNode) -> bool {
    match &node.node_type {
        ASTNodeType::Function { name, args } => {
            let name = name.rsplit('.').next().unwrap_or(name).to_ascii_uppercase();
            matches!(
                name.as_str(),
                "IF" | "IFS" | "CHOOSE" | "SWITCH" | "IFERROR" | "IFNA"
            ) || args.iter().any(ast_contains_conditional)
        }
        ASTNodeType::Call { callee, args } => {
            ast_contains_conditional(callee) || args.iter().any(ast_contains_conditional)
        }
        ASTNodeType::UnaryOp { expr, .. } => ast_contains_conditional(expr),
        ASTNodeType::BinaryOp { left, right, .. } => {
            ast_contains_conditional(left) || ast_contains_conditional(right)
        }
        ASTNodeType::Array(rows) => rows.iter().flatten().any(ast_contains_conditional),
        ASTNodeType::Reference { .. } | ASTNodeType::Literal(_) | ASTNodeType::Omitted => false,
    }
}

pub(crate) fn scc_partition_fingerprint(components: &[Vec<VertexId>]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut normalized = components
        .iter()
        .map(|component| {
            let mut members = component.iter().map(|vertex| vertex.0).collect::<Vec<_>>();
            members.sort_unstable();
            members
        })
        .collect::<Vec<_>>();
    normalized.sort_unstable();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for component in normalized {
        component.len().hash(&mut hasher);
        component.hash(&mut hasher);
    }
    hasher.finish()
}

impl DependencyGraph {
    pub fn compact_dependency_prototype_stats(&self) -> CompactDependencyPrototypeStats {
        let symbolic_range_record_count: usize =
            self.formula_to_range_deps.values().map(Vec::len).sum();
        let range_dependent_formula_count = self.formula_to_range_deps.len();
        let stripe_membership_record_count: usize =
            self.stripe_to_dependents.values().map(FxHashSet::len).sum();
        let named_dependency_record_count: usize =
            self.vertex_to_names.values().map(Vec::len).sum();
        let table_dependency_record_count = self.table_vertex_lookup.len();
        let conditional_formula_count = self
            .vertex_formulas
            .keys()
            .filter(|vertex| {
                self.get_formula(**vertex)
                    .is_some_and(|ast| ast_contains_conditional(&ast))
            })
            .count();
        let spill_anchor_count = self.spill_anchor_to_cells.len();
        let cross_sheet_dependency_record_count = self
            .vertex_formulas
            .keys()
            .map(|&dependent| {
                let dependent_sheet = self.get_vertex_sheet_id(dependent);
                self.get_dependencies(dependent)
                    .into_iter()
                    .filter(|dependency| {
                        self.get_cell_ref(*dependency)
                            .is_some_and(|cell| cell.sheet_id != dependent_sheet)
                    })
                    .count()
            })
            .sum();
        let dynamic_dependency_descriptor_count = self
            .store
            .all_vertices()
            .filter(|vertex| self.store.is_dynamic(*vertex))
            .count();
        let compact_record_count = symbolic_range_record_count
            .saturating_add(stripe_membership_record_count)
            .saturating_add(named_dependency_record_count)
            .saturating_add(table_dependency_record_count)
            .saturating_add(dynamic_dependency_descriptor_count);
        let estimated_compact_bytes = range_dependent_formula_count
            .saturating_mul(std::mem::size_of::<(VertexId, Vec<SharedRangeRef<'static>>)>())
            .saturating_add(
                symbolic_range_record_count
                    .saturating_mul(std::mem::size_of::<SharedRangeRef<'static>>()),
            )
            .saturating_add(
                stripe_membership_record_count.saturating_mul(std::mem::size_of::<VertexId>()),
            )
            .saturating_add(
                named_dependency_record_count.saturating_mul(std::mem::size_of::<VertexId>()),
            )
            .saturating_add(
                table_dependency_record_count.saturating_mul(std::mem::size_of::<VertexId>()),
            );
        CompactDependencyPrototypeStats {
            expanded_graph_edges: self.edges.num_edges_exact(),
            formula_vertices: self.vertex_formulas.len(),
            range_dependent_formula_count,
            symbolic_range_record_count,
            stripe_membership_record_count,
            named_dependency_record_count,
            table_dependency_record_count,
            conditional_formula_count,
            spill_anchor_count,
            cross_sheet_dependency_record_count,
            dynamic_dependency_descriptor_count,
            compact_record_count,
            estimated_compact_bytes,
        }
    }

    fn symbolic_scc_probe_impl(
        &self,
        virtual_dependencies: Option<&FxHashMap<VertexId, Vec<VertexId>>>,
    ) -> CompactSymbolicSccStats {
        let mut vertices = self.store.all_vertices().collect::<Vec<_>>();
        vertices.sort_unstable_by_key(|vertex| vertex.0);
        let mut positions = FxHashMap::default();
        for (position, vertex) in vertices.iter().copied().enumerate() {
            positions.insert(vertex, position);
        }
        let mut adjacency = vec![Vec::<usize>::new(); vertices.len()];
        let mut direct_edge_count = 0usize;
        let mut range_descriptor_count = 0usize;
        let mut range_neighbor_visits = 0usize;
        for (position, vertex) in vertices.iter().copied().enumerate() {
            let neighbors = &mut adjacency[position];
            let direct = self.get_dependencies(vertex);
            direct_edge_count = direct_edge_count.saturating_add(direct.len());
            neighbors.extend(
                direct
                    .into_iter()
                    .filter_map(|dependency| positions.get(&dependency).copied()),
            );
            if let Some(extra) = virtual_dependencies.and_then(|deps| deps.get(&vertex)) {
                neighbors.extend(
                    extra
                        .iter()
                        .filter_map(|dependency| positions.get(dependency).copied()),
                );
            }
            if let Some(ranges) = self.formula_to_range_deps.get(&vertex) {
                range_descriptor_count = range_descriptor_count.saturating_add(ranges.len());
                for range in ranges {
                    let sheet = self
                        .sheet_reg
                        .resolve_locator(&range.sheet, self.get_vertex_sheet_id(vertex))
                        .unwrap_or(self.get_vertex_sheet_id(vertex));
                    let bounds = self.compressed_range_resolved_bounds(
                        sheet,
                        (
                            range.start_row.map(|bound| bound.index),
                            range.end_row.map(|bound| bound.index),
                            range.start_col.map(|bound| bound.index),
                            range.end_col.map(|bound| bound.index),
                        ),
                    );
                    let Some((start_row, end_row, start_col, end_col)) = bounds else {
                        continue;
                    };
                    let range_vertices =
                        self.vertices_in_region(sheet, start_row, end_row, start_col, end_col);
                    range_neighbor_visits =
                        range_neighbor_visits.saturating_add(range_vertices.len());
                    neighbors.extend(
                        range_vertices
                            .into_iter()
                            .filter_map(|dependency| positions.get(&dependency).copied()),
                    );
                }
            }
            neighbors.sort_unstable();
            neighbors.dedup();
        }
        let transient_expanded_edge_count: usize = adjacency.iter().map(Vec::len).sum();
        let max_logical_out_degree = adjacency.iter().map(Vec::len).max().unwrap_or(0);
        let mut reverse = vec![Vec::<usize>::new(); vertices.len()];
        for (from, neighbors) in adjacency.iter().enumerate() {
            for &to in neighbors {
                reverse[to].push(from);
            }
        }
        let mut visited = vec![false; vertices.len()];
        let mut order = Vec::with_capacity(vertices.len());
        for root in 0..vertices.len() {
            if visited[root] {
                continue;
            }
            visited[root] = true;
            let mut stack = vec![(root, 0usize)];
            while let Some((node, next)) = stack.last_mut() {
                if *next < adjacency[*node].len() {
                    let child = adjacency[*node][*next];
                    *next += 1;
                    if !visited[child] {
                        visited[child] = true;
                        stack.push((child, 0));
                    }
                } else {
                    order.push(*node);
                    stack.pop();
                }
            }
        }
        let mut assigned = vec![false; vertices.len()];
        let mut scc_count = 0;
        let mut cyclic_scc_count = 0;
        let mut largest_scc_size = 0;
        let mut largest_cyclic_scc_size = 0;
        let mut component_signatures = Vec::with_capacity(vertices.len());
        for &root in order.iter().rev() {
            if assigned[root] {
                continue;
            }
            assigned[root] = true;
            let mut stack = vec![root];
            let mut component = Vec::new();
            while let Some(node) = stack.pop() {
                component.push(node);
                for &child in &reverse[node] {
                    if !assigned[child] {
                        assigned[child] = true;
                        stack.push(child);
                    }
                }
            }
            let cyclic = component.len() > 1 || adjacency[component[0]].contains(&component[0]);
            component_signatures.push(
                component
                    .iter()
                    .map(|&index| vertices[index])
                    .collect::<Vec<_>>(),
            );
            scc_count += 1;
            largest_scc_size = largest_scc_size.max(component.len());
            if cyclic {
                cyclic_scc_count += 1;
                largest_cyclic_scc_size = largest_cyclic_scc_size.max(component.len());
            }
        }
        let transient_bytes_estimate = adjacency
            .len()
            .saturating_mul(std::mem::size_of::<Vec<usize>>() * 2)
            .saturating_add(
                transient_expanded_edge_count.saturating_mul(std::mem::size_of::<usize>() * 2),
            );
        CompactSymbolicSccStats {
            vertex_count: vertices.len(),
            direct_edge_count,
            range_descriptor_count,
            range_neighbor_visits,
            transient_expanded_edge_count,
            max_logical_out_degree,
            scc_count,
            cyclic_scc_count,
            largest_scc_size,
            largest_cyclic_scc_size,
            scc_partition_fingerprint: scc_partition_fingerprint(&component_signatures),
            transient_bytes_estimate,
        }
    }

    pub fn symbolic_scc_probe(&self) -> CompactSymbolicSccStats {
        self.symbolic_scc_probe_impl(None)
    }

    pub(crate) fn symbolic_scc_probe_with_virtual(
        &self,
        virtual_dependencies: &FxHashMap<VertexId, Vec<VertexId>>,
    ) -> CompactSymbolicSccStats {
        self.symbolic_scc_probe_impl(Some(virtual_dependencies))
    }

    pub fn validate_compact_dependency_prototype(&self) -> CompactDependencyPrototypeValidation {
        let mut validation = CompactDependencyPrototypeValidation::default();
        for &dependent in self.vertex_formulas.keys() {
            let ranges = self.formula_to_range_deps.get(&dependent);
            let names = self.vertex_to_names.get(&dependent);
            for prerequisite in self.get_dependencies(dependent) {
                validation.expanded_formula_edges += 1;
                let range_match = ranges.is_some_and(|ranges| {
                    let Some(cell) = self.get_cell_ref(prerequisite) else {
                        return false;
                    };
                    ranges.iter().any(|range| {
                        let range_sheet = self
                            .sheet_reg
                            .resolve_locator(&range.sheet, self.get_vertex_sheet_id(dependent))
                            .ok();
                        range_sheet == Some(cell.sheet_id)
                            && range
                                .start_row
                                .is_none_or(|bound| cell.coord.row() >= bound.index)
                            && range
                                .end_row
                                .is_none_or(|bound| cell.coord.row() <= bound.index)
                            && range
                                .start_col
                                .is_none_or(|bound| cell.coord.col() >= bound.index)
                            && range
                                .end_col
                                .is_none_or(|bound| cell.coord.col() <= bound.index)
                    })
                });
                if range_match {
                    validation.symbolic_range_edges += 1;
                } else if names.is_some_and(|names| names.contains(&prerequisite)) {
                    validation.named_edges += 1;
                } else if self.table_vertex_lookup.contains_key(&prerequisite) {
                    validation.table_edges += 1;
                } else if self.get_cell_ref(prerequisite).is_some() {
                    validation.direct_cell_edges += 1;
                } else {
                    validation.unclassified_edges += 1;
                    let kind = format!("{:?}", self.store.kind(prerequisite));
                    if let Some((_, count)) = validation
                        .unclassified_kind_counts
                        .iter_mut()
                        .find(|(known, _)| known == &kind)
                    {
                        *count += 1;
                    } else {
                        validation.unclassified_kind_counts.push((kind.clone(), 1));
                    }
                    if validation.unclassified_samples.len() < 64 {
                        validation
                            .unclassified_samples
                            .push((dependent.0, prerequisite.0, kind));
                    }
                }
            }
        }
        validation
    }

    fn range_contains_cell(
        &self,
        dependent: VertexId,
        range: &SharedRangeRef<'static>,
        sheet_id: SheetId,
        row0: u32,
        col0: u32,
    ) -> bool {
        let range_sheet = self
            .sheet_reg
            .resolve_locator(&range.sheet, self.get_vertex_sheet_id(dependent))
            .unwrap_or(sheet_id);
        range_sheet == sheet_id
            && range.start_row.is_none_or(|bound| row0 >= bound.index)
            && range.end_row.is_none_or(|bound| row0 <= bound.index)
            && range.start_col.is_none_or(|bound| col0 >= bound.index)
            && range.end_col.is_none_or(|bound| col0 <= bound.index)
    }

    fn expanded_range_dependents_for_cell(
        &self,
        sheet_id: SheetId,
        row0: u32,
        col0: u32,
    ) -> FxHashSet<VertexId> {
        self.formula_to_range_deps
            .iter()
            .filter_map(|(&dependent, ranges)| {
                ranges
                    .iter()
                    .any(|range| self.range_contains_cell(dependent, range, sheet_id, row0, col0))
                    .then_some(dependent)
            })
            .collect()
    }

    fn dirty_closure_for_cell(
        &self,
        start: VertexId,
        use_compact_ranges: bool,
    ) -> FxHashSet<VertexId> {
        let mut visited = FxHashSet::default();
        let mut pending = vec![start];
        while let Some(vertex) = pending.pop() {
            if !visited.insert(vertex) {
                continue;
            }
            pending.extend(self.get_dependents(vertex));
            if let Some(names) = self.cell_to_name_dependents.get(&vertex) {
                pending.extend(names.iter().copied());
            }
            if self.get_cell_ref(vertex).is_some() {
                let range_dependents = if use_compact_ranges {
                    self.collect_range_dependents_for_vertex(vertex)
                } else {
                    let position = self.store.grid_addr(vertex).unwrap();
                    self.expanded_range_dependents_for_cell(
                        self.store.sheet_id(vertex),
                        position.row(),
                        position.col(),
                    )
                    .into_iter()
                    .collect()
                };
                pending.extend(range_dependents);
            }
        }
        visited
    }

    pub fn compact_dirty_set_parity(
        &self,
        sheet_id: SheetId,
        row0: u32,
        col0: u32,
    ) -> CompactDirtySetParity {
        let Some(start) = self.cell_to_vertex.iter().find_map(|(cell, &vertex)| {
            (cell.sheet_id == sheet_id && cell.coord.row() == row0 && cell.coord.col() == col0)
                .then_some(vertex)
        }) else {
            return CompactDirtySetParity {
                exact: true,
                ..CompactDirtySetParity::default()
            };
        };
        let compact = self.dirty_closure_for_cell(start, true);
        let oracle = self.dirty_closure_for_cell(start, false);
        let missing_from_compact = oracle.difference(&compact).count();
        let extra_in_compact = compact.difference(&oracle).count();
        CompactDirtySetParity {
            compact_count: compact.len(),
            oracle_count: oracle.len(),
            missing_from_compact,
            extra_in_compact,
            exact: missing_from_compact == 0 && extra_in_compact == 0,
        }
    }

    pub(crate) fn has_compressed_range_dependencies(&self) -> bool {
        !self.formula_to_range_deps.is_empty()
    }

    pub(crate) fn structural_occupancy(&self, sheet_id: SheetId) -> StructuralOccupancy {
        let mut occupancy = StructuralOccupancy::default();
        for (id, coord) in self.grid_vertices_in_sheet(sheet_id) {
            if self.store.kind(id) != VertexKind::Empty {
                occupancy.occupied_rows.push(coord.row());
                occupancy.occupied_columns.push(coord.col());
            }
        }
        occupancy.finish();
        occupancy
    }

    pub(crate) fn compressed_range_dependents_for_structural_edit(
        &self,
        sheet_id: SheetId,
        edit: StructuralEdit,
        occupancy: &StructuralOccupancy,
    ) -> Vec<VertexId> {
        self.formula_to_range_deps
            .iter()
            .filter_map(|(&dependent, ranges)| {
                ranges
                    .iter()
                    .any(|range| {
                        // `Current` is the dependent formula's own sheet. An
                        // unresolvable sheet keeps the candidate conservative.
                        let range_sheet_id = self
                            .sheet_reg
                            .resolve_locator(&range.sheet, self.get_vertex_sheet_id(dependent))
                            .ok();
                        if range_sheet_id.is_some_and(|resolved| resolved != sheet_id) {
                            return false;
                        }
                        let start_row = range.start_row.map(|bound| bound.index).unwrap_or(0);
                        let end_row = range.end_row.map(|bound| bound.index).unwrap_or(u32::MAX);
                        let start_col = range.start_col.map(|bound| bound.index).unwrap_or(0);
                        let end_col = range.end_col.map(|bound| bound.index).unwrap_or(u32::MAX);
                        let axis_matches = match edit {
                            StructuralEdit::DeleteRows { start, end } => {
                                start_row <= end && end_row >= start
                            }
                            StructuralEdit::InsertRows { before } => {
                                (range.start_row.is_none() || start_row < before)
                                    && before <= end_row
                            }
                            StructuralEdit::DeleteColumns { start, end } => {
                                start_col <= end && end_col >= start
                            }
                            StructuralEdit::InsertColumns { before } => {
                                (range.start_col.is_none() || start_col < before)
                                    && before <= end_col
                            }
                        };
                        let (cross_start, cross_end) = match edit {
                            StructuralEdit::InsertRows { .. }
                            | StructuralEdit::DeleteRows { .. } => (start_col, end_col),
                            StructuralEdit::InsertColumns { .. }
                            | StructuralEdit::DeleteColumns { .. } => (start_row, end_row),
                        };
                        axis_matches
                            && (range_sheet_id.is_none()
                                // An unresolvable sheet candidate must remain
                                // conservative; occupancy from the edited sheet
                                // cannot prove that candidate empty.
                                || StructuralOccupancy::cross_axis_occupied(
                                    occupancy,
                                    edit,
                                    cross_start,
                                    cross_end,
                                ))
                    })
                    .then_some(dependent)
            })
            .collect()
    }

    /// Visit compressed-range formula dependents covering one cell without
    /// materializing the stripe union used by dirty propagation.
    ///
    /// This path is intentionally parallel to
    /// `collect_range_dependents_for_rect`: scheduling keeps its existing
    /// behavior, while inspection can stop before a pathological stripe has
    /// been copied into an unbounded candidate set. Work is charged for every
    /// stripe candidate and every compressed range exact-check.
    pub(crate) fn visit_range_dependents_covering_bounded(
        &self,
        sheet_id: SheetId,
        row0: u32,
        col0: u32,
        remaining_work: &mut u64,
        visitor: &mut dyn FnMut(VertexId) -> bool,
    ) -> bool {
        if self.stripe_to_dependents.is_empty() {
            return true;
        }

        let mut seen = FxHashSet::default();
        let keys = [
            StripeKey {
                sheet_id,
                stripe_type: StripeType::Column,
                index: col0,
            },
            StripeKey {
                sheet_id,
                stripe_type: StripeType::Row,
                index: row0,
            },
            StripeKey {
                sheet_id,
                stripe_type: StripeType::Block,
                index: block_index(row0, col0),
            },
        ];

        for key in keys {
            if key.stripe_type == StripeType::Block && !self.config.enable_block_stripes {
                continue;
            }
            let Some(candidates) = self.stripe_to_dependents.get(&key) else {
                continue;
            };
            for &dependent in candidates {
                if *remaining_work == 0 {
                    return false;
                }
                *remaining_work -= 1;
                if !seen.insert(dependent) {
                    continue;
                }
                let Some(ranges) = self.formula_to_range_deps.get(&dependent) else {
                    continue;
                };
                let mut covered = false;
                for range in ranges {
                    if *remaining_work == 0 {
                        return false;
                    }
                    *remaining_work -= 1;
                    // `Current` is the dependent formula's own sheet; an
                    // unresolvable sheet name is interpreted on the query sheet
                    // so the dependent is not silently dropped.
                    let range_sheet = self
                        .sheet_reg
                        .resolve_locator(&range.sheet, self.get_vertex_sheet_id(dependent))
                        .unwrap_or(sheet_id);
                    if range_sheet != sheet_id {
                        continue;
                    }
                    let start_row = range.start_row.map(|bound| bound.index).unwrap_or(0);
                    let end_row = range.end_row.map(|bound| bound.index).unwrap_or(u32::MAX);
                    let start_col = range.start_col.map(|bound| bound.index).unwrap_or(0);
                    let end_col = range.end_col.map(|bound| bound.index).unwrap_or(u32::MAX);
                    if start_row <= row0 && row0 <= end_row && start_col <= col0 && col0 <= end_col
                    {
                        covered = true;
                        break;
                    }
                }
                if covered && !visitor(dependent) {
                    return false;
                }
            }
        }
        true
    }

    /// Public wrapper to add range-dependent edges.
    pub fn add_range_edges(
        &mut self,
        dependent: VertexId,
        ranges: &[SharedRangeRef<'static>],
        current_sheet_id: SheetId,
    ) {
        self.add_range_dependent_edges(dependent, ranges, current_sheet_id);
    }

    /// Return the compressed range dependencies recorded for a formula vertex, if any.
    /// These are `SharedRangeRef` entries that were not expanded into explicit
    /// cell edges due to `range_expansion_limit` or due to infinite/partial bounds.
    pub fn get_range_dependencies(
        &self,
        vertex: VertexId,
    ) -> Option<&Vec<SharedRangeRef<'static>>> {
        self.formula_to_range_deps.get(&vertex)
    }

    #[cfg(test)]
    pub(crate) fn formula_to_range_deps(
        &self,
    ) -> &FxHashMap<VertexId, Vec<SharedRangeRef<'static>>> {
        &self.formula_to_range_deps
    }

    #[cfg(test)]
    pub(crate) fn stripe_to_dependents(&self) -> &FxHashMap<StripeKey, FxHashSet<VertexId>> {
        &self.stripe_to_dependents
    }

    /// True when a (possibly open-ended) range region on `sheet_id` covers
    /// the formula vertex's own cell. Used to record a self-loop for
    /// stripe-compressed / whole-axis self-inclusion (#120): such references
    /// never produce explicit cell edges, so the ingest self-reference check
    /// (which scans expanded cell deps) misses them. `None` bounds mean the
    /// axis is unbounded (whole column/row), which always covers the cell.
    fn range_region_contains_self(
        &self,
        dependent: VertexId,
        sheet_id: SheetId,
        s_row: Option<u32>,
        e_row: Option<u32>,
        s_col: Option<u32>,
        e_col: Option<u32>,
    ) -> bool {
        if self.store.sheet_id(dependent) != sheet_id {
            return false;
        }
        // A symbol vertex has no position, so no range region can contain it.
        let Some(coord) = self.store.grid_addr(dependent) else {
            return false;
        };
        let r0 = coord.row();
        let c0 = coord.col();
        s_row.is_none_or(|s| r0 >= s)
            && e_row.is_none_or(|e| r0 <= e)
            && s_col.is_none_or(|s| c0 >= s)
            && e_col.is_none_or(|e| c0 <= e)
    }

    /// Record a self-loop edge (vertex → itself). The edge store and Tarjan
    /// both treat self-loops as cycles (`separate_cycles` via `has_self_loop`).
    fn record_self_loop(&mut self, vertex: VertexId) {
        if !self.has_self_loop(vertex) {
            self.edges.add_edge(vertex, vertex);
        }
    }

    pub(crate) fn compressed_range_resolved_bounds(
        &self,
        sheet: SheetId,
        range: (Option<u32>, Option<u32>, Option<u32>, Option<u32>),
    ) -> Option<(u32, u32, u32, u32)> {
        let (start_row, end_row, start_col, end_col) = range;
        let extent = resolve_used_extent(
            OpenRangeBounds {
                start_row,
                start_column: start_col,
                end_row,
                end_column: end_col,
            },
            ExtentPolicy::GraphCompat {
                fallback_row: self.config.max_open_ended_rows.saturating_sub(1),
                fallback_column: self.config.max_open_ended_cols.saturating_sub(1),
            },
            |first, last| self.used_row_bounds_for_columns(sheet, first, last),
            |first, last| self.used_col_bounds_for_rows(sheet, first, last),
        )?;
        Some((
            extent.start_row,
            extent.end_row,
            extent.start_column,
            extent.end_column,
        ))
    }

    /// Classify whether every occurrence of one compressed range that covers
    /// the formula cell is narrowed away from that cell by a statically
    /// resolvable `INDEX`. The range dependency itself remains conservative so
    /// used-bound growth still invalidates the formula; only the synthetic #120
    /// self-loop is omitted when the selected reference cannot contain the
    /// formula cell.
    fn compressed_range_self_use(
        &self,
        dependent: VertexId,
        range_sheet: SheetId,
        range: (Option<u32>, Option<u32>, Option<u32>, Option<u32>),
    ) -> RangeSelfUse {
        let Some(ast) = self.get_formula(dependent) else {
            return RangeSelfUse::IncludedOrUnknown;
        };

        fn static_index(node: &ASTNode) -> Option<i64> {
            match &node.node_type {
                ASTNodeType::Literal(LiteralValue::Int(value)) => Some(*value),
                ASTNodeType::Literal(LiteralValue::Number(value)) if value.is_finite() => {
                    Some(*value as i64)
                }
                ASTNodeType::UnaryOp { op, expr } if op == "+" => static_index(expr),
                ASTNodeType::UnaryOp { op, expr } if op == "-" => static_index(expr)?.checked_neg(),
                _ => None,
            }
        }

        fn matching_range(
            graph: &DependencyGraph,
            node: &ASTNode,
            dependent: VertexId,
            range_sheet: SheetId,
            range: (Option<u32>, Option<u32>, Option<u32>, Option<u32>),
        ) -> bool {
            let ASTNodeType::Reference {
                reference:
                    ReferenceType::Range {
                        sheet,
                        start_row,
                        start_col,
                        end_row,
                        end_col,
                        ..
                    },
                ..
            } = &node.node_type
            else {
                return false;
            };
            let sheet_id = match sheet.as_deref() {
                Some(name) => match graph.sheet_id(name) {
                    Some(id) => id,
                    None => return false,
                },
                None => graph.get_vertex_sheet_id(dependent),
            };
            sheet_id == range_sheet
                && start_row.map(|index| index.saturating_sub(1)) == range.0
                && end_row.map(|index| index.saturating_sub(1)) == range.1
                && start_col.map(|index| index.saturating_sub(1)) == range.2
                && end_col.map(|index| index.saturating_sub(1)) == range.3
        }

        fn selected_region_contains_self(
            graph: &DependencyGraph,
            dependent: VertexId,
            range_sheet: SheetId,
            range: (Option<u32>, Option<u32>, Option<u32>, Option<u32>),
            position: i64,
            explicit_col: Option<i64>,
        ) -> Option<bool> {
            let (sr, er, sc, ec) = graph.compressed_range_resolved_bounds(range_sheet, range)?;
            let (row, col) = match explicit_col {
                Some(col) => (position, col),
                None if sr == er => (1, position),
                None => (position, 1),
            };
            if row < 0 || col < 0 {
                return Some(false);
            }
            // A symbol vertex has no position, so no range region can contain it.
            let coord = graph.store.grid_addr(dependent)?;
            let contains = if row == 0 && col == 0 {
                coord.row() >= sr && coord.row() <= er && coord.col() >= sc && coord.col() <= ec
            } else if col == 0 {
                let selected_row = sr.checked_add(u32::try_from(row).ok()?.saturating_sub(1))?;
                selected_row <= er
                    && coord.row() == selected_row
                    && coord.col() >= sc
                    && coord.col() <= ec
            } else if row == 0 {
                let selected_col = sc.checked_add(u32::try_from(col).ok()?.saturating_sub(1))?;
                selected_col <= ec
                    && coord.col() == selected_col
                    && coord.row() >= sr
                    && coord.row() <= er
            } else {
                let selected_row = sr.checked_add(u32::try_from(row).ok()?.saturating_sub(1))?;
                let selected_col = sc.checked_add(u32::try_from(col).ok()?.saturating_sub(1))?;
                selected_row <= er
                    && selected_col <= ec
                    && coord.row() == selected_row
                    && coord.col() == selected_col
            };
            Some(contains)
        }

        fn visit(
            graph: &DependencyGraph,
            node: &ASTNode,
            dependent: VertexId,
            range_sheet: SheetId,
            range: (Option<u32>, Option<u32>, Option<u32>, Option<u32>),
            index: Option<(i64, Option<i64>)>,
        ) -> RangeSelfUse {
            if matching_range(graph, node, dependent, range_sheet, range) {
                return match index.and_then(|(row, col)| {
                    selected_region_contains_self(graph, dependent, range_sheet, range, row, col)
                }) {
                    Some(false) => RangeSelfUse::Excluded,
                    Some(true) | None => RangeSelfUse::IncludedOrUnknown,
                };
            }
            match &node.node_type {
                ASTNodeType::Function { name, args }
                    if name.eq_ignore_ascii_case("INDEX") && (2..=3).contains(&args.len()) =>
                {
                    let row = static_index(&args[1]);
                    let col = args.get(2).and_then(static_index);
                    let selection = row.and_then(|row| {
                        if args.len() == 2 || col.is_some() {
                            Some((row, col))
                        } else {
                            None
                        }
                    });
                    let mut use_kind =
                        visit(graph, &args[0], dependent, range_sheet, range, selection);
                    for arg in &args[1..] {
                        use_kind =
                            use_kind.merge(visit(graph, arg, dependent, range_sheet, range, None));
                    }
                    use_kind
                }
                ASTNodeType::Function { args, .. } => {
                    args.iter().fold(RangeSelfUse::NoMatch, |kind, arg| {
                        kind.merge(visit(graph, arg, dependent, range_sheet, range, None))
                    })
                }
                ASTNodeType::UnaryOp { expr, .. } => {
                    visit(graph, expr, dependent, range_sheet, range, None)
                }
                ASTNodeType::BinaryOp { left, right, .. } => visit(
                    graph,
                    left,
                    dependent,
                    range_sheet,
                    range,
                    None,
                )
                .merge(visit(graph, right, dependent, range_sheet, range, None)),
                ASTNodeType::Call { callee, args } => {
                    let mut kind = visit(graph, callee, dependent, range_sheet, range, None);
                    for arg in args {
                        kind = kind.merge(visit(graph, arg, dependent, range_sheet, range, None));
                    }
                    kind
                }
                ASTNodeType::Array(rows) => {
                    rows.iter()
                        .flatten()
                        .fold(RangeSelfUse::NoMatch, |kind, item| {
                            kind.merge(visit(graph, item, dependent, range_sheet, range, None))
                        })
                }
                ASTNodeType::Literal(_) | ASTNodeType::Omitted | ASTNodeType::Reference { .. } => {
                    RangeSelfUse::NoMatch
                }
            }
        }

        visit(self, &ast, dependent, range_sheet, range, None)
    }

    pub(super) fn add_range_dependent_edges(
        &mut self,
        dependent: VertexId,
        ranges: &[SharedRangeRef<'static>],
        current_sheet_id: SheetId,
    ) {
        if ranges.is_empty() {
            return;
        }

        self.formula_to_range_deps
            .insert(dependent, ranges.to_vec());

        for range in ranges {
            // `current_sheet_id` is the dependent formula's sheet, which is what
            // `Current` means. An unresolvable sheet name falls back to it so a
            // stripe is still registered rather than the edge being dropped.
            let sheet_id = self
                .sheet_reg
                .resolve_locator(&range.sheet, current_sheet_id)
                .unwrap_or(current_sheet_id);

            let s_row = range.start_row.map(|b| b.index);
            let e_row = range.end_row.map(|b| b.index);
            let s_col = range.start_col.map(|b| b.index);
            let e_col = range.end_col.map(|b| b.index);

            // #120: a compressed range whose region covers this formula's own
            // cell is a self-reference. Record a self-loop so SCC detection
            // flags the cycle (the ingest self-ref check only sees expanded
            // cell edges, which compressed ranges do not produce).
            if self.range_region_contains_self(dependent, sheet_id, s_row, e_row, s_col, e_col)
                && self.compressed_range_self_use(dependent, sheet_id, (s_row, e_row, s_col, e_col))
                    != RangeSelfUse::Excluded
            {
                self.record_self_loop(dependent);
            }

            // #376: an all-unbounded range means "the whole sheet". The stripe
            // classification below would treat it as both column- and
            // row-striped, fall through both branches, and collapse it to a
            // single row-0 stripe, hiding edits anywhere else from this
            // dependent. Register full column coverage instead; the precision
            // check against `formula_to_range_deps` already treats the missing
            // bounds as unbounded.
            if s_row.is_none() && e_row.is_none() && s_col.is_none() && e_col.is_none() {
                self.register_whole_sheet_stripes(dependent, sheet_id);
                continue;
            }

            let col_stripes = (s_row.is_none() && e_row.is_none())
                || (s_col.is_some() && e_col.is_some() && (s_row.is_none() || e_row.is_none()));
            let row_stripes = (s_col.is_none() && e_col.is_none())
                || (s_row.is_some() && e_row.is_some() && (s_col.is_none() || e_col.is_none()));

            if col_stripes && !row_stripes {
                let sc = s_col.unwrap_or(0);
                let ec = e_col.unwrap_or(sc);
                for col in sc..=ec {
                    let key = StripeKey {
                        sheet_id,
                        stripe_type: StripeType::Column,
                        index: col,
                    };
                    self.stripe_to_dependents
                        .entry(key.clone())
                        .or_default()
                        .insert(dependent);
                    #[cfg(test)]
                    {
                        if self.stripe_to_dependents.get(&key).map(|s| s.len()) == Some(1)
                            && let Ok(mut g) = self.instr.lock()
                        {
                            g.stripe_inserts += 1;
                        }
                    }
                }
                continue;
            }

            if row_stripes && !col_stripes {
                let sr = s_row.unwrap_or(0);
                let er = e_row.unwrap_or(sr);
                for row in sr..=er {
                    let key = StripeKey {
                        sheet_id,
                        stripe_type: StripeType::Row,
                        index: row,
                    };
                    self.stripe_to_dependents
                        .entry(key.clone())
                        .or_default()
                        .insert(dependent);
                    #[cfg(test)]
                    {
                        if self.stripe_to_dependents.get(&key).map(|s| s.len()) == Some(1)
                            && let Ok(mut g) = self.instr.lock()
                        {
                            g.stripe_inserts += 1;
                        }
                    }
                }
                continue;
            }

            let start_row = s_row.unwrap_or(0);
            let start_col = s_col.unwrap_or(0);
            let end_row = e_row.unwrap_or(start_row);
            let end_col = e_col.unwrap_or(start_col);

            let height = end_row.saturating_sub(start_row) + 1;
            let width = end_col.saturating_sub(start_col) + 1;

            if self.config.enable_block_stripes && height > 1 && width > 1 {
                let start_block_row = start_row / BLOCK_H;
                let end_block_row = end_row / BLOCK_H;
                let start_block_col = start_col / BLOCK_W;
                let end_block_col = end_col / BLOCK_W;

                for block_row in start_block_row..=end_block_row {
                    for block_col in start_block_col..=end_block_col {
                        let key = StripeKey {
                            sheet_id,
                            stripe_type: StripeType::Block,
                            index: block_index(block_row * BLOCK_H, block_col * BLOCK_W),
                        };
                        self.stripe_to_dependents
                            .entry(key.clone())
                            .or_default()
                            .insert(dependent);
                        #[cfg(test)]
                        {
                            if self.stripe_to_dependents.get(&key).map(|s| s.len()) == Some(1)
                                && let Ok(mut g) = self.instr.lock()
                            {
                                g.stripe_inserts += 1;
                            }
                        }
                    }
                }
            } else if height > width {
                for col in start_col..=end_col {
                    let key = StripeKey {
                        sheet_id,
                        stripe_type: StripeType::Column,
                        index: col,
                    };
                    self.stripe_to_dependents
                        .entry(key.clone())
                        .or_default()
                        .insert(dependent);
                    #[cfg(test)]
                    {
                        if self.stripe_to_dependents.get(&key).map(|s| s.len()) == Some(1)
                            && let Ok(mut g) = self.instr.lock()
                        {
                            g.stripe_inserts += 1;
                        }
                    }
                }
            } else {
                for row in start_row..=end_row {
                    let key = StripeKey {
                        sheet_id,
                        stripe_type: StripeType::Row,
                        index: row,
                    };
                    self.stripe_to_dependents
                        .entry(key.clone())
                        .or_default()
                        .insert(dependent);
                    #[cfg(test)]
                    {
                        if self.stripe_to_dependents.get(&key).map(|s| s.len()) == Some(1)
                            && let Ok(mut g) = self.instr.lock()
                        {
                            g.stripe_inserts += 1;
                        }
                    }
                }
            }
        }
    }

    /// Register stripes covering every cell of a sheet, for a dependent whose
    /// range is unbounded on both axes (#376). Dirty-propagation lookups probe
    /// the column stripe of every edited cell, so covering all columns
    /// guarantees any edit on the sheet reaches the precision check.
    fn register_whole_sheet_stripes(&mut self, dependent: VertexId, sheet_id: SheetId) {
        /// Excel sheet column capacity (column XFD), as a 0-based exclusive bound.
        const SHEET_MAX_COLS: u32 = 16_384;
        for col in 0..SHEET_MAX_COLS {
            let key = StripeKey {
                sheet_id,
                stripe_type: StripeType::Column,
                index: col,
            };
            self.stripe_to_dependents
                .entry(key)
                .or_default()
                .insert(dependent);
        }
    }

    /// Fast-path: add range dependencies using compact RangeKey.
    pub fn add_range_deps_from_keys(
        &mut self,
        dependent: VertexId,
        keys: &[crate::engine::plan::RangeKey],
        current_sheet_id: SheetId,
    ) {
        use crate::engine::plan::RangeKey as RK;
        if keys.is_empty() {
            return;
        }

        let mut shared_ranges: Vec<SharedRangeRef<'static>> = Vec::with_capacity(keys.len());
        for k in keys {
            let sheet_loc = SharedSheetLocator::Id(match k {
                RK::Rect { sheet, .. }
                | RK::WholeRow { sheet, .. }
                | RK::WholeCol { sheet, .. }
                | RK::OpenRect { sheet, .. } => *sheet,
            });

            let mk_axis = |idx0: u32| formualizer_common::AxisBound::new(idx0, false);

            let built = match k {
                RK::Rect { start, end, .. } => {
                    let sr = mk_axis(start.row());
                    let sc = mk_axis(start.col());
                    let er = mk_axis(end.row());
                    let ec = mk_axis(end.col());
                    SharedRangeRef::from_parts(sheet_loc, Some(sr), Some(sc), Some(er), Some(ec))
                        .ok()
                }
                RK::WholeRow { row, .. } => {
                    let r0 = row.saturating_sub(1);
                    let b = mk_axis(r0);
                    SharedRangeRef::from_parts(sheet_loc, Some(b), None, Some(b), None).ok()
                }
                RK::WholeCol { col, .. } => {
                    let c0 = col.saturating_sub(1);
                    let b = mk_axis(c0);
                    SharedRangeRef::from_parts(sheet_loc, None, Some(b), None, Some(b)).ok()
                }
                RK::OpenRect {
                    start_row,
                    start_col,
                    end_row,
                    end_col,
                    ..
                } => SharedRangeRef::from_parts(
                    sheet_loc,
                    start_row.map(mk_axis),
                    start_col.map(mk_axis),
                    end_row.map(mk_axis),
                    end_col.map(mk_axis),
                )
                .ok(),
            };

            if let Some(r) = built {
                shared_ranges.push(r.into_owned());
            }
        }

        if shared_ranges.is_empty() {
            return;
        }

        self.formula_to_range_deps
            .insert(dependent, shared_ranges.clone());

        for range in &shared_ranges {
            // See add_range_dependent_edges.
            let sheet_id = self
                .sheet_reg
                .resolve_locator(&range.sheet, current_sheet_id)
                .unwrap_or(current_sheet_id);

            let s_row = range.start_row.map(|b| b.index);
            let e_row = range.end_row.map(|b| b.index);
            let s_col = range.start_col.map(|b| b.index);
            let e_col = range.end_col.map(|b| b.index);

            // #120: see add_range_dependent_edges — compressed range covering
            // the formula's own cell records a self-loop for SCC detection.
            if self.range_region_contains_self(dependent, sheet_id, s_row, e_row, s_col, e_col)
                && self.compressed_range_self_use(dependent, sheet_id, (s_row, e_row, s_col, e_col))
                    != RangeSelfUse::Excluded
            {
                self.record_self_loop(dependent);
            }

            // #376: an all-unbounded range means "the whole sheet". The stripe
            // classification below would treat it as both column- and
            // row-striped, fall through both branches, and collapse it to a
            // single row-0 stripe, hiding edits anywhere else from this
            // dependent. Register full column coverage instead; the precision
            // check against `formula_to_range_deps` already treats the missing
            // bounds as unbounded.
            if s_row.is_none() && e_row.is_none() && s_col.is_none() && e_col.is_none() {
                self.register_whole_sheet_stripes(dependent, sheet_id);
                continue;
            }

            let col_stripes = (s_row.is_none() && e_row.is_none())
                || (s_col.is_some() && e_col.is_some() && (s_row.is_none() || e_row.is_none()));
            let row_stripes = (s_col.is_none() && e_col.is_none())
                || (s_row.is_some() && e_row.is_some() && (s_col.is_none() || e_col.is_none()));

            if col_stripes && !row_stripes {
                let sc = s_col.unwrap_or(0);
                let ec = e_col.unwrap_or(sc);
                for col in sc..=ec {
                    let key = StripeKey {
                        sheet_id,
                        stripe_type: StripeType::Column,
                        index: col,
                    };
                    self.stripe_to_dependents
                        .entry(key)
                        .or_default()
                        .insert(dependent);
                }
                continue;
            }

            if row_stripes && !col_stripes {
                let sr = s_row.unwrap_or(0);
                let er = e_row.unwrap_or(sr);
                for row in sr..=er {
                    let key = StripeKey {
                        sheet_id,
                        stripe_type: StripeType::Row,
                        index: row,
                    };
                    self.stripe_to_dependents
                        .entry(key)
                        .or_default()
                        .insert(dependent);
                }
                continue;
            }

            let start_row = s_row.unwrap_or(0);
            let start_col = s_col.unwrap_or(0);
            let end_row = e_row.unwrap_or(start_row);
            let end_col = e_col.unwrap_or(start_col);

            let height = end_row.saturating_sub(start_row) + 1;
            let width = end_col.saturating_sub(start_col) + 1;

            if self.config.enable_block_stripes && height > 1 && width > 1 {
                let start_block_row = start_row / BLOCK_H;
                let end_block_row = end_row / BLOCK_H;
                let start_block_col = start_col / BLOCK_W;
                let end_block_col = end_col / BLOCK_W;

                for block_row in start_block_row..=end_block_row {
                    for block_col in start_block_col..=end_block_col {
                        let key = StripeKey {
                            sheet_id,
                            stripe_type: StripeType::Block,
                            index: block_index(block_row * BLOCK_H, block_col * BLOCK_W),
                        };
                        self.stripe_to_dependents
                            .entry(key)
                            .or_default()
                            .insert(dependent);
                    }
                }
            } else if height > width {
                for col in start_col..=end_col {
                    let key = StripeKey {
                        sheet_id,
                        stripe_type: StripeType::Column,
                        index: col,
                    };
                    self.stripe_to_dependents
                        .entry(key)
                        .or_default()
                        .insert(dependent);
                }
            } else {
                for row in start_row..=end_row {
                    let key = StripeKey {
                        sheet_id,
                        stripe_type: StripeType::Row,
                        index: row,
                    };
                    self.stripe_to_dependents
                        .entry(key)
                        .or_default()
                        .insert(dependent);
                }
            }
        }
    }
}
