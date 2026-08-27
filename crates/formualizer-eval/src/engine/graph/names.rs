use super::*;
use formualizer_common::parse_a1_1based;

#[inline]
fn normalize_name_key(name: &str) -> String {
    name.to_lowercase()
}

fn formula_is_array(ast: &ASTNode) -> bool {
    match &ast.node_type {
        ASTNodeType::Reference { reference, .. } => matches!(
            reference,
            ReferenceType::Range { .. }
                | ReferenceType::Range3D { .. }
                | ReferenceType::Table(_)
                | ReferenceType::External(_)
        ),
        ASTNodeType::Array(_) => true,
        ASTNodeType::UnaryOp { op, expr } => op != "@" && formula_is_array(expr),
        ASTNodeType::BinaryOp { op, left, right } => {
            op == ":" || formula_is_array(left) || formula_is_array(right)
        }
        ASTNodeType::Function { name, .. } => {
            crate::function_registry::get("", name).is_some_and(|function| {
                function.caps().intersects(
                    crate::function::FnCaps::RETURNS_REFERENCE | crate::function::FnCaps::MAY_SPILL,
                )
            })
        }
        ASTNodeType::Call { .. } | ASTNodeType::Literal(_) | ASTNodeType::Omitted => false,
    }
}

/// Validate that a name conforms to Excel naming rules.
fn is_valid_excel_name(name: &str) -> bool {
    // Excel name rules:
    // 1. Must start with a letter, underscore, or backslash
    // 2. Can contain letters, numbers, periods, and underscores
    // 3. Cannot be a cell reference (like A1, B2, etc.)
    // 4. Cannot exceed 255 characters
    // 5. Cannot contain spaces

    if name.is_empty() || name.len() > 255 {
        return false;
    }

    if parse_a1_1based(name).is_ok() {
        return false;
    }

    let mut chars = name.chars();

    // First character must be letter, underscore, or backslash
    if let Some(first) = chars.next()
        && !first.is_alphabetic()
        && first != '_'
        && first != '\\'
    {
        return false;
    }

    // Remaining characters must be letters, digits, periods, or underscores
    for c in chars {
        if !c.is_alphanumeric() && c != '.' && c != '_' {
            return false;
        }
    }

    true
}

/// Helper function to adjust a named definition during structural operations.
///
/// Named definitions track structural edits regardless of `$` anchors, matching
/// formula references. Absolute markers affect copy/fill, not structural shifts.
fn adjust_named_definition(
    definition: &mut NamedDefinition,
    adjuster: &crate::engine::graph::editor::reference_adjuster::ReferenceAdjuster,
    operation: &crate::engine::graph::editor::reference_adjuster::ShiftOperation,
    context: &crate::engine::graph::editor::reference_adjuster::ReferenceContext<'_>,
) -> Result<(), ExcelError> {
    use crate::engine::graph::editor::reference_adjuster::AbsShiftPolicy;
    let mut invalidated = false;
    match definition {
        NamedDefinition::Cell(cell_ref) => {
            if let Some(adjusted) =
                adjuster.adjust_cell_ref_with_policy(cell_ref, operation, AbsShiftPolicy::Track)
            {
                *cell_ref = adjusted;
            } else {
                invalidated = true;
            }
        }
        NamedDefinition::Range(range_ref) => {
            let adjusted_start = adjuster.adjust_cell_ref_with_policy(
                &range_ref.start,
                operation,
                AbsShiftPolicy::Track,
            );
            let adjusted_end = adjuster.adjust_cell_ref_with_policy(
                &range_ref.end,
                operation,
                AbsShiftPolicy::Track,
            );

            if let (Some(start), Some(end)) = (adjusted_start, adjusted_end) {
                range_ref.start = start;
                range_ref.end = end;
            } else {
                invalidated = true;
            }
        }
        NamedDefinition::Literal(_) => {
            // Constant names are not affected by structural shifts.
        }
        NamedDefinition::Formula {
            ast,
            dependencies,
            range_deps,
        } => {
            let adjusted_ast = adjuster.adjust_ast_with_policy_in_context(
                ast,
                operation,
                AbsShiftPolicy::Track,
                context,
            );
            *ast = adjusted_ast;

            dependencies.clear();
            range_deps.clear();
        }
    }
    if invalidated {
        *definition = NamedDefinition::Formula {
            ast: formualizer_parse::parser::ASTNode::new(
                formualizer_parse::parser::ASTNodeType::Literal(LiteralValue::Error(
                    ExcelError::new(ExcelErrorKind::Ref),
                )),
                None,
            ),
            dependencies: Vec::new(),
            range_deps: Vec::new(),
        };
    }
    Ok(())
}

impl DependencyGraph {
    #[inline]
    pub(crate) fn name_lookup_key(&self, name: &str) -> String {
        if self.config.case_sensitive_names {
            name.to_string()
        } else {
            normalize_name_key(name)
        }
    }

    fn canonical_name_in_scope(&self, scope: NameScope, name: &str) -> Option<String> {
        let key = self.name_lookup_key(name);
        match scope {
            NameScope::Workbook => self.named_ranges_lookup.get(&key).cloned(),
            NameScope::Sheet(sheet_id) => self
                .sheet_named_ranges_lookup
                .get(&(sheet_id, key))
                .cloned(),
        }
    }

    /// Allocate the next address in the symbol space.
    ///
    /// Symbols are identified by name and have no position. They used to be handed
    /// fabricated grid coordinates on a real sheet, which let grid operations reach them
    /// (#302, #304); a `SymbolAddr` is not a position and cannot be reached that way.
    pub(super) fn next_symbol_addr(&mut self) -> VertexAddr {
        let seq = self.symbol_vertex_seq;
        self.symbol_vertex_seq = self.symbol_vertex_seq.wrapping_add(1);
        VertexAddr::symbol(SymbolAddr::new(seq))
    }

    /// Allocate a vertex in the symbol address space.
    ///
    /// `scope_sheet_id` is recorded as lookup metadata only: it says which scope the symbol
    /// answers queries for, never where it lives. Symbol vertices are absent from
    /// `cell_to_vertex` and from every sheet index by construction, because they have no
    /// grid address to key them by.
    pub(super) fn allocate_symbol_vertex(
        &mut self,
        kind: VertexKind,
        scope_sheet_id: SheetId,
    ) -> VertexId {
        let addr = self.next_symbol_addr();
        let vertex_id = self.store.allocate(addr, scope_sheet_id, 0x01);
        self.store.set_kind(vertex_id, kind);
        self.edges.add_vertex(addr, vertex_id.0);
        self.note_symbol_vertex_allocation();
        vertex_id
    }

    pub(super) fn allocate_name_vertex(&mut self, scope: NameScope) -> VertexId {
        // Scope is lookup metadata, not an address: a workbook-scoped name is not a
        // resident of the default sheet.
        let scope_sheet_id = match scope {
            NameScope::Sheet(id) => id,
            NameScope::Workbook => self.default_sheet_id,
        };
        let vertex_id = self.allocate_symbol_vertex(VertexKind::NamedScalar, scope_sheet_id);
        self.mark_vertex_dirty(vertex_id);
        vertex_id
    }

    // Named Range Methods

    pub(crate) fn validate_define_name(
        &self,
        name: &str,
        scope: NameScope,
    ) -> Result<(), ExcelError> {
        if !is_valid_excel_name(name) {
            return Err(
                ExcelError::new(ExcelErrorKind::Name).with_message(format!("Invalid name: {name}"))
            );
        }

        let lookup_key = self.name_lookup_key(name);
        match scope {
            NameScope::Workbook => {
                if let Some(existing) = self.named_ranges_lookup.get(&lookup_key) {
                    return Err(ExcelError::new(ExcelErrorKind::Name).with_message(format!(
                        "Name collision under normalization: '{name}' conflicts with '{existing}'"
                    )));
                }
            }
            NameScope::Sheet(sheet_id) => {
                if let Some(existing) = self.sheet_named_ranges_lookup.get(&(sheet_id, lookup_key))
                {
                    return Err(ExcelError::new(ExcelErrorKind::Name).with_message(format!(
                        "Name collision under normalization in sheet: '{name}' conflicts with '{existing}'"
                    )));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validate_existing_name(
        &self,
        name: &str,
        scope: NameScope,
    ) -> Result<(), ExcelError> {
        self.canonical_name_in_scope(scope, name)
            .map(|_| ())
            .ok_or_else(|| {
                ExcelError::new(ExcelErrorKind::Name)
                    .with_message(format!("Name not found: {name}"))
            })
    }

    /// Define a new named range
    pub fn define_name(
        &mut self,
        name: &str,
        definition: NamedDefinition,
        scope: NameScope,
    ) -> Result<(), ExcelError> {
        self.validate_define_name(name, scope)?;

        let formula_dependencies = if let NamedDefinition::Formula { ref ast, .. } = definition {
            let started = std::time::Instant::now();
            let result = self.extract_dependencies_with_pending_names(
                ast,
                match scope {
                    NameScope::Sheet(id) => id,
                    NameScope::Workbook => self.default_sheet_id,
                },
            )?;
            self.name_definition_stats
                .formula_dependency_extraction_calls = self
                .name_definition_stats
                .formula_dependency_extraction_calls
                .saturating_add(1);
            self.name_definition_stats.formula_dependency_extraction_ns = self
                .name_definition_stats
                .formula_dependency_extraction_ns
                .saturating_add(started.elapsed().as_nanos());
            Some(result)
        } else {
            None
        };

        let final_definition = match &definition {
            NamedDefinition::Formula { ast, .. } => {
                let Some((dependencies, range_deps, _, _, _)) = formula_dependencies.as_ref()
                else {
                    unreachable!();
                };
                NamedDefinition::Formula {
                    ast: ast.clone(),
                    dependencies: dependencies.clone(),
                    range_deps: range_deps.clone(),
                }
            }
            _ => definition.clone(),
        };

        let publication_started = std::time::Instant::now();
        let vertex_id = self.allocate_name_vertex(scope);

        let named_range = NamedRange {
            definition: final_definition,
            scope,
            dependents: FxHashSet::default(),
            vertex: vertex_id,
        };

        let is_array = match &named_range.definition {
            NamedDefinition::Range(_) => true,
            NamedDefinition::Formula { ast, .. } => formula_is_array(ast),
            NamedDefinition::Cell(_) | NamedDefinition::Literal(_) => false,
        };
        self.store.set_kind(
            vertex_id,
            if is_array {
                VertexKind::NamedArray
            } else {
                VertexKind::NamedScalar
            },
        );
        let definition_for_rebuild = match &named_range.definition {
            NamedDefinition::Formula { .. } => None,
            _ => Some(named_range.definition.clone()),
        };

        let key = name.to_string();
        match scope {
            NameScope::Workbook => {
                self.named_ranges.insert(key.clone(), named_range);
                self.named_ranges_lookup
                    .insert(self.name_lookup_key(&key), key.clone());
            }
            NameScope::Sheet(id) => {
                self.sheet_named_ranges
                    .insert((id, key.clone()), named_range);
                self.sheet_named_ranges_lookup
                    .insert((id, self.name_lookup_key(&key)), key.clone());
            }
        }
        self.name_vertex_lookup.insert(vertex_id, (scope, key));
        self.name_definition_stats.name_index_insertions = self
            .name_definition_stats
            .name_index_insertions
            .saturating_add(3);

        if let Some((dependencies, range_deps, _, referenced_names, pending_names)) =
            formula_dependencies
        {
            if !dependencies.is_empty() {
                self.add_dependent_edges(vertex_id, &dependencies);
            }
            self.register_name_cell_dependencies(vertex_id, &dependencies);
            if !range_deps.is_empty() {
                let sheet_id = match scope {
                    NameScope::Sheet(id) => id,
                    NameScope::Workbook => self.default_sheet_id,
                };
                self.add_range_dependent_edges(vertex_id, &range_deps, sheet_id);
            }
            if !referenced_names.is_empty() {
                self.attach_vertex_to_names(vertex_id, &referenced_names);
            }
            let sheet_id = match scope {
                NameScope::Sheet(id) => id,
                NameScope::Workbook => self.default_sheet_id,
            };
            for pending_name in pending_names {
                self.record_pending_name_reference(sheet_id, &pending_name, vertex_id);
            }
        } else {
            let definition_for_rebuild = definition_for_rebuild.expect("non-formula definition");
            let referenced_names =
                self.rebuild_name_dependencies(vertex_id, &definition_for_rebuild, scope)?;
            if !referenced_names.is_empty() {
                self.attach_vertex_to_names(vertex_id, &referenced_names);
            }
        }

        if self.symbol_revision_batch_depth == 0 {
            self.resolve_pending_name_references(scope, name);
        }
        self.bump_symbol_revision();
        self.name_definition_stats.publication_ns = self
            .name_definition_stats
            .publication_ns
            .saturating_add(publication_started.elapsed().as_nanos());

        Ok(())
    }

    /// Iterate workbook-scoped named ranges (for bindings/testing)
    pub fn named_ranges_iter(&self) -> impl Iterator<Item = (&String, &NamedRange)> {
        self.named_ranges.iter()
    }

    /// Iterate sheet-scoped named ranges (for bindings/testing)
    pub fn sheet_named_ranges_iter(
        &self,
    ) -> impl Iterator<Item = (&(SheetId, String), &NamedRange)> {
        self.sheet_named_ranges.iter()
    }

    /// Resolve a name in an explicit [`NameScope`].
    ///
    /// [`NameScope::Sheet`] looks in that sheet's names first and falls back to
    /// workbook scope, matching Excel's shadowing rules. [`NameScope::Workbook`]
    /// looks in workbook-scoped names **only**: a sheet-scoped name is invisible
    /// to a workbook-scope query even when it is scoped to the default sheet.
    ///
    /// This is the single owned derivation for name scoping. A caller with no
    /// sheet context asks for [`NameScope::Workbook`], never for the default
    /// sheet's scope - substituting the default sheet for missing context is
    /// what leaked references onto unrelated sheets in issue #110.
    pub fn resolve_name_entry_in_scope(&self, name: &str, scope: NameScope) -> Option<&NamedRange> {
        let workbook_entry = || {
            if self.config.case_sensitive_names {
                self.named_ranges.get(name)
            } else {
                self.named_ranges_lookup
                    .get(&self.name_lookup_key(name))
                    .and_then(|canon| self.named_ranges.get(canon))
            }
        };

        match scope {
            NameScope::Workbook => workbook_entry(),
            NameScope::Sheet(current_sheet) => {
                if self.config.case_sensitive_names {
                    self.sheet_named_ranges
                        .get(&(current_sheet, name.to_string()))
                        .or_else(workbook_entry)
                } else {
                    let key = self.name_lookup_key(name);
                    self.sheet_named_ranges_lookup
                        .get(&(current_sheet, key))
                        .and_then(|canon| {
                            self.sheet_named_ranges.get(&(current_sheet, canon.clone()))
                        })
                        .or_else(workbook_entry)
                }
            }
        }
    }

    /// Resolve a name as seen from `current_sheet`: sheet scope shadows
    /// workbook scope. Equivalent to [`Self::resolve_name_entry_in_scope`] with
    /// [`NameScope::Sheet`].
    pub fn resolve_name_entry(&self, name: &str, current_sheet: SheetId) -> Option<&NamedRange> {
        self.resolve_name_entry_in_scope(name, NameScope::Sheet(current_sheet))
    }

    /// Resolve a named range to its definition
    pub fn resolve_name(&self, name: &str, current_sheet: SheetId) -> Option<&NamedDefinition> {
        self.resolve_name_entry(name, current_sheet)
            .map(|nr| &nr.definition)
    }

    /// The folded lookup key (see [`Self::name_lookup_key`]) of the name
    /// represented by `vertex`, if it is a name vertex. Used by SCC tasks for
    /// deterministic member ordering and live name-read matching (RFC #112).
    pub(crate) fn name_key_for_vertex(&self, vertex: VertexId) -> Option<String> {
        self.name_vertex_lookup
            .get(&vertex)
            .map(|(_, name)| self.name_lookup_key(name))
    }

    pub fn named_range_by_vertex(&self, vertex: VertexId) -> Option<&NamedRange> {
        self.name_vertex_lookup
            .get(&vertex)
            .and_then(|(scope, name)| match scope {
                NameScope::Workbook => self.named_ranges.get(name),
                NameScope::Sheet(sheet_id) => {
                    self.sheet_named_ranges.get(&(*sheet_id, name.clone()))
                }
            })
    }

    /// Update an existing named range definition
    pub fn update_name(
        &mut self,
        name: &str,
        new_definition: NamedDefinition,
        scope: NameScope,
    ) -> Result<(), ExcelError> {
        let Some(canon_name) = self.canonical_name_in_scope(scope, name) else {
            return Err(ExcelError::new(ExcelErrorKind::Name)
                .with_message(format!("Name not found: {name}")));
        };

        // First collect dependents to avoid borrow checker issues
        let dependents_to_dirty = match scope {
            NameScope::Workbook => self
                .named_ranges
                .get(&canon_name)
                .map(|nr| nr.dependents.iter().copied().collect::<Vec<_>>()),
            NameScope::Sheet(id) => self
                .sheet_named_ranges
                .get(&(id, canon_name.clone()))
                .map(|nr| nr.dependents.iter().copied().collect::<Vec<_>>()),
        };

        if let Some(dependents) = dependents_to_dirty {
            // Mark all dependents as dirty
            for vertex_id in dependents {
                self.mark_vertex_dirty(vertex_id);
            }

            // Now update the definition
            let named_range = match scope {
                NameScope::Workbook => self.named_ranges.get_mut(&canon_name),
                NameScope::Sheet(id) => self.sheet_named_ranges.get_mut(&(id, canon_name.clone())),
            };

            let mut update_data: Option<(VertexId, NameScope, NamedDefinition, bool)> = None;
            if let Some(named_range) = named_range {
                named_range.definition = new_definition;
                let is_range = match &named_range.definition {
                    NamedDefinition::Range(_) => true,
                    NamedDefinition::Formula { ast, .. } => formula_is_array(ast),
                    NamedDefinition::Cell(_) | NamedDefinition::Literal(_) => false,
                };
                update_data = Some((
                    named_range.vertex,
                    named_range.scope,
                    named_range.definition.clone(),
                    is_range,
                ));
            }

            if let Some((vertex, scope_value, definition_snapshot, is_range)) = update_data {
                self.detach_vertex_from_names(vertex);

                if is_range {
                    self.store.set_kind(vertex, VertexKind::NamedArray);
                } else {
                    self.store.set_kind(vertex, VertexKind::NamedScalar);
                }
                self.mark_vertex_dirty(vertex);

                let referenced_names =
                    self.rebuild_name_dependencies(vertex, &definition_snapshot, scope_value)?;
                if !referenced_names.is_empty() {
                    self.attach_vertex_to_names(vertex, &referenced_names);
                }
            }

            self.bump_symbol_revision();
            Ok(())
        } else {
            Err(ExcelError::new(ExcelErrorKind::Name)
                .with_message(format!("Name not found: {name}")))
        }
    }

    /// Delete a named range
    pub fn delete_name(&mut self, name: &str, scope: NameScope) -> Result<(), ExcelError> {
        let Some(canon_name) = self.canonical_name_in_scope(scope, name) else {
            return Err(ExcelError::new(ExcelErrorKind::Name)
                .with_message(format!("Name not found: {name}")));
        };

        let named_range = match scope {
            NameScope::Workbook => {
                let removed = self.named_ranges.remove(&canon_name);
                let key = self.name_lookup_key(&canon_name);
                self.named_ranges_lookup.remove(&key);
                removed
            }
            NameScope::Sheet(id) => {
                let removed = self.sheet_named_ranges.remove(&(id, canon_name.clone()));
                let key = self.name_lookup_key(&canon_name);
                self.sheet_named_ranges_lookup.remove(&(id, key));
                removed
            }
        };

        if let Some(named_range) = named_range {
            let mut affected: FxHashSet<VertexId> = FxHashSet::default();
            for &vertex_id in &named_range.dependents {
                affected.insert(vertex_id);
            }
            for (vertex_id, names) in self.vertex_to_names.iter() {
                if names.contains(&named_range.vertex) {
                    affected.insert(*vertex_id);
                }
            }
            for vertex_id in affected {
                self.mark_vertex_dirty(vertex_id);
                if let Some(names) = self.vertex_to_names.get_mut(&vertex_id) {
                    names.retain(|vid| *vid != named_range.vertex);
                    if names.is_empty() {
                        self.vertex_to_names.remove(&vertex_id);
                    }
                }
            }
            self.mark_named_vertex_deleted(&named_range);
            self.bump_symbol_revision();
            Ok(())
        } else {
            Err(ExcelError::new(ExcelErrorKind::Name)
                .with_message(format!("Name not found: {name}")))
        }
    }

    pub(super) fn detach_vertex_from_names(&mut self, vertex: VertexId) {
        if let Some(prior) = self.vertex_to_names.remove(&vertex) {
            for name_vertex in prior {
                if let Some((scope, name)) = self.name_vertex_lookup.get(&name_vertex).cloned() {
                    match scope {
                        NameScope::Workbook => {
                            if let Some(entry) = self.named_ranges.get_mut(&name) {
                                entry.dependents.remove(&vertex);
                            }
                        }
                        NameScope::Sheet(sheet_id) => {
                            if let Some(entry) =
                                self.sheet_named_ranges.get_mut(&(sheet_id, name.clone()))
                            {
                                entry.dependents.remove(&vertex);
                            }
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn attach_vertex_to_names(&mut self, vertex: VertexId, names: &[VertexId]) {
        if names.is_empty() {
            return;
        }
        let mut unique = FxHashSet::default();
        let mut recorded = Vec::new();
        for &name_vertex in names {
            if !unique.insert(name_vertex) {
                continue;
            }
            if let Some((scope, name)) = self.name_vertex_lookup.get(&name_vertex).cloned() {
                match scope {
                    NameScope::Workbook => {
                        if let Some(entry) = self.named_ranges.get_mut(&name) {
                            entry.dependents.insert(vertex);
                        }
                    }
                    NameScope::Sheet(sheet_id) => {
                        if let Some(entry) =
                            self.sheet_named_ranges.get_mut(&(sheet_id, name.clone()))
                        {
                            entry.dependents.insert(vertex);
                        }
                    }
                }
                recorded.push(name_vertex);
            }
        }
        if !recorded.is_empty() {
            self.vertex_to_names.insert(vertex, recorded);
        }
    }

    pub(super) fn unregister_name_cell_dependencies(&mut self, name_vertex: VertexId) {
        if let Some(prev) = self.name_to_cell_dependencies.remove(&name_vertex) {
            for dep in prev {
                if let Some(set) = self.cell_to_name_dependents.get_mut(&dep) {
                    set.remove(&name_vertex);
                    if set.is_empty() {
                        self.cell_to_name_dependents.remove(&dep);
                    }
                }
            }
        }
    }

    pub(super) fn register_name_cell_dependencies(
        &mut self,
        name_vertex: VertexId,
        dependencies: &[VertexId],
    ) {
        self.unregister_name_cell_dependencies(name_vertex);
        if dependencies.is_empty() {
            return;
        }
        for dep in dependencies {
            self.cell_to_name_dependents
                .entry(*dep)
                .or_default()
                .insert(name_vertex);
        }
        self.name_to_cell_dependencies
            .insert(name_vertex, dependencies.to_vec());
    }

    pub(crate) fn record_pending_name_reference(
        &mut self,
        sheet_id: SheetId,
        name: &str,
        formula_vertex: VertexId,
    ) {
        let key = self.name_lookup_key(name);
        self.pending_name_links
            .entry(key.clone())
            .or_default()
            .insert((sheet_id, formula_vertex));
        self.vertex_to_pending_names
            .entry(formula_vertex)
            .or_default()
            .insert(key);
    }

    pub(crate) fn clear_pending_name_references(&mut self, formula_vertex: VertexId) {
        let Some(keys) = self.vertex_to_pending_names.remove(&formula_vertex) else {
            return;
        };

        for key in keys {
            let mut remove_key = false;
            if let Some(entries) = self.pending_name_links.get_mut(&key) {
                entries.retain(|(_, vertex_id)| *vertex_id != formula_vertex);
                remove_key = entries.is_empty();
            }
            if remove_key {
                self.pending_name_links.remove(&key);
            }
        }
    }

    fn clear_pending_name_reference_key(&mut self, formula_vertex: VertexId, key: &str) {
        let mut remove_vertex = false;
        if let Some(keys) = self.vertex_to_pending_names.get_mut(&formula_vertex) {
            keys.remove(key);
            remove_vertex = keys.is_empty();
        }
        if remove_vertex {
            self.vertex_to_pending_names.remove(&formula_vertex);
        }
    }

    pub(super) fn resolve_pending_name_references(&mut self, scope: NameScope, name: &str) {
        let key = self.name_lookup_key(name);
        if let Some(entries) = self.pending_name_links.remove(&key) {
            for (sheet_id, formula_vertex) in entries {
                let attach = match scope {
                    NameScope::Workbook => true,
                    NameScope::Sheet(expected) => expected == sheet_id,
                };
                if attach {
                    if let Some(ast) = self.get_formula(formula_vertex) {
                        self.rebuild_formula_dependencies(formula_vertex, &ast);
                    } else if let Some(named_vertex) = self
                        .canonical_name_in_scope(scope, name)
                        .and_then(|canonical| match scope {
                            NameScope::Workbook => self
                                .named_ranges
                                .get(&canonical)
                                .map(|named_range| named_range.vertex),
                            NameScope::Sheet(scope_sheet) => self
                                .sheet_named_ranges
                                .get(&(scope_sheet, canonical))
                                .map(|named_range| named_range.vertex),
                        })
                    {
                        self.add_dependent_edges(formula_vertex, &[named_vertex]);
                        self.attach_vertex_to_names(formula_vertex, &[named_vertex]);
                        self.clear_pending_name_reference_key(formula_vertex, &key);
                    } else {
                        self.clear_pending_name_references(formula_vertex);
                    }
                } else {
                    self.record_pending_name_reference(sheet_id, name, formula_vertex);
                }
            }
        }
    }

    pub(crate) fn resolve_pending_name_references_for_load(&mut self) -> usize {
        let mut resolved = std::collections::BTreeMap::<VertexId, Vec<VertexId>>::new();
        let mut resolved_pending = Vec::new();
        for (name, entries) in &self.pending_name_links {
            for &(sheet_id, formula_vertex) in entries {
                let Some(named_vertex) = self
                    .resolve_name_entry(name, sheet_id)
                    .map(|named_range| named_range.vertex)
                else {
                    continue;
                };
                resolved
                    .entry(formula_vertex)
                    .or_default()
                    .push(named_vertex);
                resolved_pending.push((name.clone(), sheet_id, formula_vertex));
            }
        }

        let resolved_pending_count = resolved_pending.len();
        self.edges.begin_batch();
        for (formula_vertex, mut dependencies) in resolved {
            dependencies.sort_unstable_by_key(|vertex| vertex.0);
            dependencies.dedup();
            let has_cycle = dependencies.iter().any(|name_vertex| {
                let mut visited = FxHashSet::default();
                self.name_depends_on_vertex(*name_vertex, formula_vertex, &mut visited)
            });
            if has_cycle {
                self.mark_as_ref_error(formula_vertex);
                continue;
            }
            self.ref_error_vertices.remove(&formula_vertex);
            self.attach_vertex_to_names(formula_vertex, &dependencies);
            self.add_dependent_edges_nobatch(formula_vertex, &dependencies);
        }
        self.edges.end_batch();

        for (name, sheet_id, formula_vertex) in resolved_pending {
            let mut remove_name = false;
            if let Some(entries) = self.pending_name_links.get_mut(&name) {
                entries.remove(&(sheet_id, formula_vertex));
                remove_name = entries.is_empty();
            }
            if remove_name {
                self.pending_name_links.remove(&name);
            }
            self.clear_pending_name_reference_key(formula_vertex, &name);
        }
        resolved_pending_count
    }

    pub(super) fn name_depends_on_vertex(
        &self,
        name_vertex: VertexId,
        target: VertexId,
        visited: &mut FxHashSet<VertexId>,
    ) -> bool {
        if !visited.insert(name_vertex) {
            return false;
        }

        for dependency in self.edges.out_edges(name_vertex).iter().copied() {
            if dependency == target {
                return true;
            }

            if matches!(
                self.store.kind(dependency),
                VertexKind::NamedScalar | VertexKind::NamedArray
            ) && self.name_depends_on_vertex(dependency, target, visited)
            {
                return true;
            }
        }

        false
    }

    pub(super) fn rebuild_name_dependencies(
        &mut self,
        vertex: VertexId,
        definition: &NamedDefinition,
        scope: NameScope,
    ) -> Result<Vec<VertexId>, ExcelError> {
        let formula_dependencies = if let NamedDefinition::Formula { ast, .. } = definition {
            let current_sheet_id = match scope {
                NameScope::Sheet(id) => id,
                NameScope::Workbook => self.default_sheet_id,
            };
            let (dependencies, range_dependencies, _, _, pending_names) =
                self.extract_dependencies_with_pending_names(ast, current_sheet_id)?;
            Some((dependencies, range_dependencies, pending_names))
        } else {
            None
        };

        self.remove_dependent_edges(vertex);
        self.unregister_name_cell_dependencies(vertex);
        self.clear_pending_name_references(vertex);

        let pending_names = formula_dependencies
            .as_ref()
            .map(|(_, _, pending_names)| pending_names.clone())
            .unwrap_or_default();
        let mut dependencies: Vec<VertexId> = Vec::new();
        let mut range_dependencies: Vec<SharedRangeRef<'static>> = Vec::new();
        let mut placeholders = Vec::new();

        match definition {
            NamedDefinition::Cell(cell_ref) => {
                let vertex_id = self.get_or_create_vertex(cell_ref, &mut placeholders);
                dependencies.push(vertex_id);
            }
            NamedDefinition::Range(range_ref) => {
                let height = range_ref
                    .end
                    .coord
                    .row()
                    .saturating_sub(range_ref.start.coord.row())
                    + 1;
                let width = range_ref
                    .end
                    .coord
                    .col()
                    .saturating_sub(range_ref.start.coord.col())
                    + 1;
                let size = (width * height) as usize;

                if size <= self.config.range_expansion_limit {
                    for row in range_ref.start.coord.row()..=range_ref.end.coord.row() {
                        for col in range_ref.start.coord.col()..=range_ref.end.coord.col() {
                            let coord = Coord::new(row, col, true, true);
                            let addr = CellRef::new(range_ref.start.sheet_id, coord);
                            let vertex_id = self.get_or_create_vertex(&addr, &mut placeholders);
                            dependencies.push(vertex_id);
                        }
                    }
                } else {
                    let sheet_loc = SharedSheetLocator::Id(range_ref.start.sheet_id);
                    let sr = formualizer_common::AxisBound::new(
                        range_ref.start.coord.row(),
                        range_ref.start.coord.row_abs(),
                    );
                    let sc = formualizer_common::AxisBound::new(
                        range_ref.start.coord.col(),
                        range_ref.start.coord.col_abs(),
                    );
                    let er = formualizer_common::AxisBound::new(
                        range_ref.end.coord.row(),
                        range_ref.end.coord.row_abs(),
                    );
                    let ec = formualizer_common::AxisBound::new(
                        range_ref.end.coord.col(),
                        range_ref.end.coord.col_abs(),
                    );
                    if let Ok(r) = SharedRangeRef::from_parts(
                        sheet_loc,
                        Some(sr),
                        Some(sc),
                        Some(er),
                        Some(ec),
                    ) {
                        range_dependencies.push(r.into_owned());
                    }
                }
            }
            NamedDefinition::Literal(_) => {
                // No dependencies.
            }
            NamedDefinition::Formula { .. } => {
                let Some((formula_deps, range_deps, _)) = formula_dependencies.as_ref() else {
                    return Err(ExcelError::new(ExcelErrorKind::Error)
                        .with_message("Internal error: formula dependencies were not extracted"));
                };
                dependencies.extend(formula_deps.iter().copied());
                range_dependencies.extend(range_deps.iter().cloned());
            }
        }

        if !dependencies.is_empty() {
            self.add_dependent_edges(vertex, &dependencies);
        }
        self.register_name_cell_dependencies(vertex, &dependencies);

        if !range_dependencies.is_empty() {
            let sheet_id = match scope {
                NameScope::Sheet(id) => id,
                NameScope::Workbook => self.default_sheet_id,
            };
            self.add_range_dependent_edges(vertex, &range_dependencies, sheet_id);
        }

        if !pending_names.is_empty() {
            let sheet_id = match scope {
                NameScope::Sheet(id) => id,
                NameScope::Workbook => self.default_sheet_id,
            };
            for name in pending_names {
                self.record_pending_name_reference(sheet_id, &name, vertex);
            }
        }

        Ok(dependencies
            .iter()
            .filter(|vid| {
                matches!(
                    self.store.kind(**vid),
                    VertexKind::NamedScalar | VertexKind::NamedArray
                )
            })
            .copied()
            .collect())
    }

    pub fn adjust_named_ranges(
        &mut self,
        operation: &crate::engine::graph::editor::reference_adjuster::ShiftOperation,
    ) -> Result<(), ExcelError> {
        let adjuster = crate::engine::graph::editor::reference_adjuster::ReferenceAdjuster::new();

        let changed = !self.named_ranges.is_empty() || !self.sheet_named_ranges.is_empty();
        // Workbook-scoped formulas bind unqualified references to the default sheet.
        let workbook_context =
            crate::engine::graph::editor::reference_adjuster::ReferenceContext::new(
                self.default_sheet_id,
                &self.sheet_reg,
            );
        // Adjust cloned definitions first so a future fallible definition kind
        // cannot leave the name table half-adjusted.
        let mut adjusted_named_ranges = self.named_ranges.clone();
        let mut adjusted_sheet_named_ranges = self.sheet_named_ranges.clone();
        for named_range in adjusted_named_ranges.values_mut() {
            adjust_named_definition(
                &mut named_range.definition,
                &adjuster,
                operation,
                &workbook_context,
            )?;
        }

        // Sheet-scoped formulas bind unqualified references to their scope sheet.
        for ((scope_sheet_id, _), named_range) in adjusted_sheet_named_ranges.iter_mut() {
            let context = crate::engine::graph::editor::reference_adjuster::ReferenceContext::new(
                *scope_sheet_id,
                &self.sheet_reg,
            );
            adjust_named_definition(&mut named_range.definition, &adjuster, operation, &context)?;
        }
        let changed_names: Vec<_> = adjusted_named_ranges
            .iter()
            .filter_map(|(key, adjusted)| {
                self.named_ranges
                    .get(key)
                    .is_some_and(|current| current.definition != adjusted.definition)
                    .then_some((adjusted.vertex, adjusted.scope, adjusted.definition.clone()))
            })
            .chain(
                adjusted_sheet_named_ranges
                    .iter()
                    .filter_map(|(key, adjusted)| {
                        self.sheet_named_ranges
                            .get(key)
                            .is_some_and(|current| current.definition != adjusted.definition)
                            .then_some((
                                adjusted.vertex,
                                adjusted.scope,
                                adjusted.definition.clone(),
                            ))
                    }),
            )
            .collect();
        self.named_ranges = adjusted_named_ranges;
        self.sheet_named_ranges = adjusted_sheet_named_ranges;
        for &(vertex, scope, ref definition) in &changed_names {
            self.detach_vertex_from_names(vertex);
            self.store.set_kind(
                vertex,
                if matches!(definition, NamedDefinition::Range(_)) {
                    VertexKind::NamedArray
                } else {
                    VertexKind::NamedScalar
                },
            );
            let referenced_names = self.rebuild_name_dependencies(vertex, definition, scope)?;
            if !referenced_names.is_empty() {
                self.attach_vertex_to_names(vertex, &referenced_names);
            }
        }
        self.mark_dirty_many(
            &changed_names
                .iter()
                .map(|(vertex, _, _)| *vertex)
                .collect::<Vec<_>>(),
        );
        if changed {
            self.bump_symbol_revision();
        }

        Ok(())
    }

    /// Mark a vertex as having a #NAME! error
    pub fn mark_as_name_error(&mut self, vertex_id: VertexId) {
        // Mark the vertex as dirty
        self.mark_vertex_dirty(vertex_id);
    }

    pub(super) fn mark_named_vertex_deleted(&mut self, named_range: &NamedRange) {
        self.detach_vertex_from_names(named_range.vertex);
        self.remove_dependent_edges(named_range.vertex);
        self.unregister_name_cell_dependencies(named_range.vertex);
        self.store.mark_deleted(named_range.vertex, true);
        self.vertex_values.remove(&named_range.vertex);
        self.vertex_formulas.remove(&named_range.vertex);
        self.clear_formula_vertex_dirty(named_range.vertex);
        self.volatile_vertices.remove(&named_range.vertex);
        self.vertex_to_names.remove(&named_range.vertex);
        self.name_vertex_lookup.remove(&named_range.vertex);
    }
}
