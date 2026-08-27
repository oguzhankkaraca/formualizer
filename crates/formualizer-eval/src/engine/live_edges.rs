//! Live-edge collection for statically-cyclic SCC evaluation (Stage 1 of the
//! runtime-cycle-verdicts work; pre-work for RFC #112).
//!
//! When a statically-cyclic SCC is evaluated member-by-member (Stage 2), we
//! must record which reads *actually occurred* targeting other SCC members
//! ("live edges"). Untaken short-circuit branches (`IF`/`IFS`/`CHOOSE`/
//! `SWITCH`, ...) never execute their reads, so they contribute no live edges
//! for free. After a pass, Stage 2 classifies the live subgraph: acyclic means
//! the cycle was phantom (values stand); cyclic means `#CIRC!` or iterative
//! evaluation.
//!
//! Stage 1 ships only the collection machinery:
//!
//! * [`LiveEdgeCollector`] — a per-SCC set of member cells plus the live edges
//!   observed so far.
//! * [`RecordingContext`] — a delegating [`EvaluationContext`] wrapper around
//!   `&Engine<R>` that records reads as they resolve and forwards everything
//!   else verbatim.
//!
//! # Inertness (binding constraint)
//!
//! Nothing in this module is wired into any production evaluation path. The
//! acyclic/hot evaluation path never constructs a `RecordingContext`; no
//! `Engine` field, flag, or branch was added. The wrapper is only exercised by
//! Stage-2 SCC tasks (future) and by tests, so its cost is strictly zero for
//! ordinary recalculation.
//!
//! # Threading
//!
//! SCC members are evaluated **sequentially on a single thread**; the
//! collector is never contended. Interior mutability is required because the
//! resolver traits take `&self`, and the `Send + Sync` super-bounds on
//! [`crate::traits::ReferenceResolver`] et al. rule out `RefCell`, so we use a
//! `Mutex`. It is uncontended by construction (single-threaded SCC pass), so
//! the lock is a fast path (uncontested futex acquire) and never blocks.
//!
//! # Coordinates
//!
//! The collector API uses the engine's internal convention: 0-based row and
//! column indices, rectangles **inclusive** of both corners (matching
//! [`RangeView::start_row`]/[`RangeView::end_row`] and `CellRef`'s `Coord`).
//! Resolver-level call sites (1-based Excel coordinates) convert before
//! recording.

use std::sync::Mutex;
use std::time::Instant;

use formualizer_common::{ExcelError, LiteralValue};
use formualizer_parse::parser::{ReferenceType, TableReference};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::engine::eval::Engine;
use crate::engine::range_view::RangeView;
use crate::reference::{CellRef, SheetId};
use crate::traits::{
    EvaluationContext, FunctionProvider, NamedRangeResolver, Range, RangeResolver, ReferenceInfo,
    ReferenceResolver, Resolver, SourceResolver, Table, TableResolver,
};

/* ───────────────────────── LiveEdgeCollector ───────────────────────── */

/// One SCC member cell in collector-internal form (0-based coordinates).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MemberCell {
    sheet_id: SheetId,
    row: u32,
    col: u32,
}

#[derive(Clone, Copy, Debug)]
struct IndexedMember {
    row: u32,
    col: u32,
    member_idx: u32,
}

#[derive(Default)]
struct MemberCoordinateIndex {
    by_sheet: FxHashMap<SheetId, Vec<IndexedMember>>,
}

const READ_FINGERPRINT_OFFSET: u64 = 0xcbf29ce484222325;

fn mix_read_fingerprint(current: u64, fields: &[u64]) -> u64 {
    fields.iter().fold(current, |hash, field| {
        (hash ^ field).wrapping_mul(0x100000001b3)
    })
}

fn text_fingerprint(text: &str) -> u64 {
    text.bytes().fold(READ_FINGERPRINT_OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

impl MemberCoordinateIndex {
    fn new(members: &[MemberCell]) -> Self {
        let mut by_sheet: FxHashMap<SheetId, Vec<IndexedMember>> = FxHashMap::default();
        for (member_idx, member) in members.iter().enumerate() {
            by_sheet
                .entry(member.sheet_id)
                .or_default()
                .push(IndexedMember {
                    row: member.row,
                    col: member.col,
                    member_idx: member_idx as u32,
                });
        }
        for members in by_sheet.values_mut() {
            members.sort_unstable_by_key(|member| (member.row, member.col, member.member_idx));
        }
        Self { by_sheet }
    }

    fn candidates(&self, sheet_id: SheetId, sr: u32, er: u32) -> &[IndexedMember] {
        let Some(members) = self.by_sheet.get(&sheet_id) else {
            return &[];
        };
        let start = members.partition_point(|member| member.row < sr);
        let end = members.partition_point(|member| member.row <= er);
        &members[start..end]
    }
}

#[derive(Default)]
struct CollectorState {
    /// Index (into `members`) of the member currently being evaluated.
    /// `None` until `set_current` is called; reads observed while `None` are
    /// not attributable and are dropped.
    current: Option<u32>,
    /// Live edges as `(from_member_idx, to_member_idx)`. Self-edges `(i, i)`
    /// are recorded (e.g. a member whose range argument includes itself).
    edges: FxHashSet<(u32, u32)>,
    /// Optional origin bitmask for each observed edge. This is populated only
    /// for the opt-in edge-origin diagnostic path.
    origins: FxHashMap<(u32, u32), u16>,
    legacy_edges: FxHashSet<(u32, u32)>,
    legacy_origins: FxHashMap<(u32, u32), u16>,
    read_counters: FxHashMap<u32, LiveReadCounters>,
    read_traces: FxHashMap<u32, Vec<String>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveEdgeOrigin {
    DirectCell,
    Range,
    WholeRow,
    WholeColumn,
    NamedRange,
    Table,
    DynamicReference,
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveReadCounters {
    pub scalar_reads: u64,
    pub range_reads: u64,
    pub range_cells: u64,
    pub named_reads: u64,
    pub internal_target_events: u64,
    pub range_membership_checks: u64,
    pub collection_ns: u64,
    pub read_events: u64,
    pub read_fingerprint: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemberCoordinateIndexMode {
    Legacy,
    Compare,
    #[default]
    Indexed,
}

pub fn member_coordinate_index_mode() -> MemberCoordinateIndexMode {
    #[cfg(target_arch = "wasm32")]
    {
        MemberCoordinateIndexMode::Indexed
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match std::env::var("FZ_SCC_MEMBER_COORDINATE_INDEX_MODE")
            .ok()
            .as_deref()
        {
            Some("legacy") => MemberCoordinateIndexMode::Legacy,
            Some("compare") => MemberCoordinateIndexMode::Compare,
            _ => MemberCoordinateIndexMode::Indexed,
        }
    }
}

impl LiveEdgeOrigin {
    pub fn label(self) -> &'static str {
        match self {
            Self::DirectCell => "direct_cell",
            Self::Range => "range",
            Self::WholeRow => "whole_row",
            Self::WholeColumn => "whole_column",
            Self::NamedRange => "named_range",
            Self::Table => "table",
            Self::DynamicReference => "dynamic_reference",
            Self::Other => "other",
        }
    }

    pub fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// Records which reads actually occurred targeting SCC members during a
/// sequential member-by-member evaluation pass.
///
/// * Scalar reads are O(1) (hash lookup keyed by `(sheet, row, col)`).
/// * Rectangle reads are recorded **once per resolved rect** and intersected
///   with the membership in O(|SCC|) — never per cell of the rect.
/// * Name reads (named-formula SCC members, spec §7.13) are O(1) lookups by
///   the engine-folded name key.
///
/// Member indices are split: cell members occupy `0..cell_count`, name
/// members occupy `cell_count..cell_count + name_count` (matching the spec
/// §7.13 member ordering used by SCC tasks: cells first, then names).
pub struct LiveEdgeCollector {
    /// Iterable membership for rect intersection.
    members: Vec<MemberCell>,
    member_coordinate_index: MemberCoordinateIndex,
    /// O(1) scalar lookup: (sheet_id, row0, col0) -> member index.
    index: FxHashMap<(SheetId, u32, u32), u32>,
    /// O(1) name lookup: engine-folded name key -> member index (indices
    /// start after the cell members).
    name_index: FxHashMap<String, u32>,
    /// Total member count (cells + names); valid `set_current` range.
    total_members: usize,
    /// See module docs: uncontended Mutex forced by `Send + Sync` bounds on
    /// the resolver traits; SCC passes are single-threaded.
    state: Mutex<CollectorState>,
    track_origins: bool,
    track_reads: bool,
    mode: MemberCoordinateIndexMode,
    index_build_ns: u128,
}

impl LiveEdgeCollector {
    /// Build a collector for the given SCC membership. Member order defines
    /// the indices used in recorded edges.
    pub fn new(members: &[CellRef]) -> Self {
        Self::new_with_names(members, &[])
    }

    /// Build a collector over cell members plus name-vertex members. Cell
    /// members get indices `0..cells.len()`; name member `j` gets index
    /// `cells.len() + j`. `names` must already be folded with the engine's
    /// name-folding rule (see [`Engine::fold_name_key`]).
    pub fn new_with_names(cells: &[CellRef], names: &[String]) -> Self {
        Self::new_with_names_and_origins(cells, names, false)
    }

    pub fn new_with_names_and_origins(
        cells: &[CellRef],
        names: &[String],
        track_origins: bool,
    ) -> Self {
        Self::new_with_diagnostics(cells, names, track_origins, false)
    }

    pub fn new_with_diagnostics(
        cells: &[CellRef],
        names: &[String],
        track_origins: bool,
        track_reads: bool,
    ) -> Self {
        Self::new_with_diagnostics_and_mode(
            cells,
            names,
            track_origins,
            track_reads,
            MemberCoordinateIndexMode::Indexed,
        )
    }

    pub fn new_with_diagnostics_and_mode(
        cells: &[CellRef],
        names: &[String],
        track_origins: bool,
        track_reads: bool,
        mode: MemberCoordinateIndexMode,
    ) -> Self {
        let members: Vec<MemberCell> = cells
            .iter()
            .map(|c| MemberCell {
                sheet_id: c.sheet_id,
                row: c.coord.row(),
                col: c.coord.col(),
            })
            .collect();
        let mut index = FxHashMap::default();
        index.reserve(members.len());
        for (i, m) in members.iter().enumerate() {
            index.insert((m.sheet_id, m.row, m.col), i as u32);
        }
        let mut name_index = FxHashMap::default();
        name_index.reserve(names.len());
        for (j, name) in names.iter().enumerate() {
            name_index.insert(name.clone(), (members.len() + j) as u32);
        }
        let total_members = members.len() + names.len();
        let (member_coordinate_index, index_build_ns) = match mode {
            MemberCoordinateIndexMode::Legacy => (MemberCoordinateIndex::default(), 0),
            MemberCoordinateIndexMode::Compare | MemberCoordinateIndexMode::Indexed => {
                let index_started = Instant::now();
                let index = MemberCoordinateIndex::new(&members);
                (index, index_started.elapsed().as_nanos())
            }
        };
        Self {
            members,
            member_coordinate_index,
            index,
            name_index,
            total_members,
            state: Mutex::new(CollectorState::default()),
            track_origins,
            track_reads,
            mode,
            index_build_ns,
        }
    }

    pub fn member_count(&self) -> usize {
        self.total_members
    }

    pub fn index_build_ns(&self) -> u128 {
        self.index_build_ns
    }

    /// Set the member whose formula is about to be evaluated; subsequent
    /// recorded reads are attributed to it.
    pub fn set_current(&self, member_idx: u32) {
        debug_assert!((member_idx as usize) < self.total_members);
        self.state.lock().unwrap().current = Some(member_idx);
    }

    /// Stop attributing reads to any member (used between passes so that
    /// out-of-band reads — snapshots, deltas — never record edges).
    pub fn clear_current(&self) {
        self.state.lock().unwrap().current = None;
    }

    /// Record a scalar read of `(sheet_id, row, col)` (0-based).
    pub fn record_scalar(&self, sheet_id: SheetId, row: u32, col: u32) {
        self.record_scalar_with_origin(sheet_id, row, col, LiveEdgeOrigin::DirectCell);
    }

    pub fn record_scalar_with_origin(
        &self,
        sheet_id: SheetId,
        row: u32,
        col: u32,
        origin: LiveEdgeOrigin,
    ) {
        let collection_started = self.track_reads.then(Instant::now);
        let to = self.index.get(&(sheet_id, row, col)).copied();
        let mut st = self.state.lock().unwrap();
        let Some(from) = st.current else {
            return;
        };
        if self.track_reads {
            let counters = st.read_counters.entry(from).or_default();
            counters.scalar_reads += 1;
            counters.read_events += 1;
            if to.is_some() {
                counters.internal_target_events += 1;
            }
            let fingerprint = if counters.read_fingerprint == 0 {
                READ_FINGERPRINT_OFFSET
            } else {
                counters.read_fingerprint
            };
            counters.read_fingerprint = mix_read_fingerprint(
                fingerprint,
                &[
                    1,
                    u64::from(origin.bit()),
                    u64::from(sheet_id),
                    u64::from(row),
                    u64::from(col),
                ],
            );
            let trace = st.read_traces.entry(from).or_default();
            if trace.len() < 64 {
                trace.push(format!(
                    "scalar sheet={sheet_id:?} row={row} col={col} origin={} target_member={to:?}",
                    origin.label(),
                ));
            }
        }
        if let Some(to) = to {
            st.edges.insert((from, to));
            if self.mode == MemberCoordinateIndexMode::Compare {
                st.legacy_edges.insert((from, to));
            }
            if self.track_origins {
                *st.origins.entry((from, to)).or_default() |= origin.bit();
                if self.mode == MemberCoordinateIndexMode::Compare {
                    *st.legacy_origins.entry((from, to)).or_default() |= origin.bit();
                }
            }
        }
        if let Some(started) = collection_started {
            if let Some(counters) = st.read_counters.get_mut(&from) {
                counters.collection_ns = counters
                    .collection_ns
                    .saturating_add(started.elapsed().as_nanos() as u64);
            }
        }
    }

    /// Record a rectangle read (0-based, inclusive corners). Intersection is
    /// O(|SCC|): each member is tested against the rect once; the rect is
    /// never enumerated per cell.
    pub fn record_rect(&self, sheet_id: SheetId, sr: u32, sc: u32, er: u32, ec: u32) {
        self.record_rect_with_origin(sheet_id, sr, sc, er, ec, LiveEdgeOrigin::Range);
    }

    pub fn record_rect_with_origin(
        &self,
        sheet_id: SheetId,
        sr: u32,
        sc: u32,
        er: u32,
        ec: u32,
        origin: LiveEdgeOrigin,
    ) {
        let collection_started = self.track_reads.then(Instant::now);
        let indexed_candidates = self.member_coordinate_index.candidates(sheet_id, sr, er);
        let membership_count = match self.mode {
            MemberCoordinateIndexMode::Legacy | MemberCoordinateIndexMode::Compare => {
                self.members.len() as u64
            }
            MemberCoordinateIndexMode::Indexed => indexed_candidates.len() as u64,
        };
        let mut st = self.state.lock().unwrap();
        let Some(from) = st.current else {
            return;
        };
        if self.track_reads {
            let counters = st.read_counters.entry(from).or_default();
            counters.range_reads += 1;
            counters.range_membership_checks = counters
                .range_membership_checks
                .saturating_add(membership_count);
            counters.range_cells = counters.range_cells.saturating_add(
                u64::from(er.saturating_sub(sr).saturating_add(1))
                    .saturating_mul(u64::from(ec.saturating_sub(sc).saturating_add(1))),
            );
            counters.read_events += 1;
            let fingerprint = if counters.read_fingerprint == 0 {
                READ_FINGERPRINT_OFFSET
            } else {
                counters.read_fingerprint
            };
            counters.read_fingerprint = mix_read_fingerprint(
                fingerprint,
                &[
                    2,
                    u64::from(origin.bit()),
                    u64::from(sheet_id),
                    u64::from(sr),
                    u64::from(sc),
                    u64::from(er),
                    u64::from(ec),
                ],
            );
            let trace = st.read_traces.entry(from).or_default();
            if trace.len() < 64 {
                trace.push(format!(
                    "range sheet={sheet_id:?} rows={sr}:{er} cols={sc}:{ec} origin={}",
                    origin.label(),
                ));
            }
        }
        if self.mode == MemberCoordinateIndexMode::Legacy
            || self.mode == MemberCoordinateIndexMode::Compare
        {
            for (i, m) in self.members.iter().enumerate() {
                if m.sheet_id == sheet_id
                    && m.row >= sr
                    && m.row <= er
                    && m.col >= sc
                    && m.col <= ec
                {
                    let to = i as u32;
                    if self.mode == MemberCoordinateIndexMode::Compare {
                        st.legacy_edges.insert((from, to));
                    } else {
                        st.edges.insert((from, to));
                    }
                    if self.track_origins {
                        let origins = if self.mode == MemberCoordinateIndexMode::Compare {
                            &mut st.legacy_origins
                        } else {
                            &mut st.origins
                        };
                        *origins.entry((from, to)).or_default() |= origin.bit();
                    }
                    if self.track_reads {
                        st.read_counters
                            .get_mut(&from)
                            .expect("read counters initialized above")
                            .internal_target_events += 1;
                    }
                }
            }
        }
        if self.mode == MemberCoordinateIndexMode::Indexed
            || self.mode == MemberCoordinateIndexMode::Compare
        {
            for member in indexed_candidates {
                if member.col < sc || member.col > ec {
                    continue;
                }
                let to = member.member_idx;
                st.edges.insert((from, to));
                if self.track_origins {
                    *st.origins.entry((from, to)).or_default() |= origin.bit();
                }
                if self.track_reads {
                    st.read_counters
                        .get_mut(&from)
                        .expect("read counters initialized above")
                        .internal_target_events += 1;
                }
            }
        }
        if let Some(started) = collection_started {
            if let Some(counters) = st.read_counters.get_mut(&from) {
                counters.collection_ns = counters
                    .collection_ns
                    .saturating_add(started.elapsed().as_nanos() as u64);
            }
        }
    }

    /// Record a read of a named entity by folded name key (e.g. a formula
    /// referencing a named-formula SCC member).
    pub fn record_name(&self, folded_name: &str) {
        self.record_name_with_origin(folded_name, LiveEdgeOrigin::NamedRange);
    }

    pub fn record_name_with_origin(&self, folded_name: &str, origin: LiveEdgeOrigin) {
        let collection_started = self.track_reads.then(Instant::now);
        let to = self.name_index.get(folded_name).copied();
        let mut st = self.state.lock().unwrap();
        let Some(from) = st.current else {
            return;
        };
        if self.track_reads {
            let counters = st.read_counters.entry(from).or_default();
            counters.named_reads += 1;
            counters.read_events += 1;
            if to.is_some() {
                counters.internal_target_events += 1;
            }
            let fingerprint = if counters.read_fingerprint == 0 {
                READ_FINGERPRINT_OFFSET
            } else {
                counters.read_fingerprint
            };
            counters.read_fingerprint = mix_read_fingerprint(
                fingerprint,
                &[3, u64::from(origin.bit()), text_fingerprint(folded_name)],
            );
            let trace = st.read_traces.entry(from).or_default();
            if trace.len() < 64 {
                trace.push(format!(
                    "name key={folded_name} origin={} target_member={to:?}",
                    origin.label(),
                ));
            }
        }
        if let Some(to) = to {
            st.edges.insert((from, to));
            if self.mode == MemberCoordinateIndexMode::Compare {
                st.legacy_edges.insert((from, to));
            }
            if self.track_origins {
                *st.origins.entry((from, to)).or_default() |= origin.bit();
                if self.mode == MemberCoordinateIndexMode::Compare {
                    *st.legacy_origins.entry((from, to)).or_default() |= origin.bit();
                }
            }
        }
        if let Some(started) = collection_started {
            if let Some(counters) = st.read_counters.get_mut(&from) {
                counters.collection_ns = counters
                    .collection_ns
                    .saturating_add(started.elapsed().as_nanos() as u64);
            }
        }
    }

    /// Drain the collected edges, leaving the collector empty (current member
    /// attribution is preserved).
    pub fn take_edges(&self) -> FxHashSet<(u32, u32)> {
        std::mem::take(&mut self.state.lock().unwrap().edges)
    }

    pub fn take_legacy_edges(&self) -> FxHashSet<(u32, u32)> {
        std::mem::take(&mut self.state.lock().unwrap().legacy_edges)
    }

    pub fn take_edge_origins(&self) -> FxHashMap<(u32, u32), u16> {
        std::mem::take(&mut self.state.lock().unwrap().origins)
    }

    pub fn take_legacy_edge_origins(&self) -> FxHashMap<(u32, u32), u16> {
        std::mem::take(&mut self.state.lock().unwrap().legacy_origins)
    }

    pub fn take_member_read_counters(&self, member_idx: u32) -> LiveReadCounters {
        std::mem::take(
            self.state
                .lock()
                .unwrap()
                .read_counters
                .entry(member_idx)
                .or_default(),
        )
    }

    pub fn take_member_read_trace(&self, member_idx: u32) -> Vec<String> {
        std::mem::take(
            self.state
                .lock()
                .unwrap()
                .read_traces
                .entry(member_idx)
                .or_default(),
        )
    }
}

/* ───────────────────────── RecordingContext ───────────────────────── */

/// Delegating [`EvaluationContext`] that wraps `&Engine<R>` and records reads
/// into a [`LiveEdgeCollector`].
///
/// Interception points (everything else is pure delegation):
///
/// * `EvaluationContext::resolve_cell_reference_value` — the interpreter's
///   scalar read path (current-sheet aware).
/// * `EvaluationContext::resolve_range_view` — the single choke point for
///   range, named-range, table and dynamic (`INDIRECT`/`OFFSET`) reads. The
///   engine resolves un/partially-bounded references to concrete used-region
///   bounds, and the returned view carries the resolved sheet + rect, so we
///   record exactly that rect once. Views materialised from owned rows (array
///   literals, named literals/formulas) carry the synthetic `"__tmp"` sheet,
///   which has no `SheetId`, so they are skipped automatically.
/// * `ReferenceResolver::resolve_cell_reference` — sheet-qualified scalar
///   reads (e.g. implicit intersection).
/// * `RangeResolver::resolve_range_reference` — legacy boxed-range path; the
///   rect is resolved via the engine's own `resolve_range_view` normalisation
///   so unbounded references record their used-region bounds.
///
/// Not recordable at this layer (Stage 2 follow-ups, noted in tests):
///
/// * `NamedRangeResolver::resolve_named_range_reference` — values-only API
///   with no sheet/region context. The engine-level named-range path flows
///   through `resolve_range_view` (intercepted); only the external-resolver
///   fallback is invisible.
/// * `TableResolver::resolve_table_reference` — returns an opaque `Table`.
///   Engine-registered tables flow through `resolve_range_view` (intercepted).
pub struct RecordingContext<'a, R: EvaluationContext> {
    engine: &'a Engine<R>,
    collector: &'a LiveEdgeCollector,
}

impl<'a, R: EvaluationContext> RecordingContext<'a, R> {
    pub fn new(engine: &'a Engine<R>, collector: &'a LiveEdgeCollector) -> Self {
        Self { engine, collector }
    }

    /// Record a read of a named entity, folding the raw reference text with
    /// the engine's name-folding rule so it matches collector name keys.
    fn record_name(&self, raw_name: &str) {
        let key = self.engine.graph.name_lookup_key(raw_name);
        self.collector
            .record_name_with_origin(&key, LiveEdgeOrigin::NamedRange);
    }

    fn origin_for_reference(reference: &ReferenceType) -> LiveEdgeOrigin {
        match reference {
            ReferenceType::Cell { .. } | ReferenceType::Cell3D { .. } => LiveEdgeOrigin::DirectCell,
            ReferenceType::Range {
                start_row,
                start_col,
                end_row,
                end_col,
                ..
            } => match (start_row, end_row, start_col, end_col) {
                (None, None, Some(_), Some(_)) => LiveEdgeOrigin::WholeColumn,
                (Some(_), Some(_), None, None) => LiveEdgeOrigin::WholeRow,
                (Some(_), Some(_), Some(_), Some(_)) => LiveEdgeOrigin::Range,
                _ => LiveEdgeOrigin::Other,
            },
            ReferenceType::Range3D { .. } => LiveEdgeOrigin::Range,
            ReferenceType::NamedRange(_) => LiveEdgeOrigin::NamedRange,
            ReferenceType::Table(_) => LiveEdgeOrigin::Table,
            ReferenceType::External(_) => LiveEdgeOrigin::Other,
        }
    }

    /// Record a scalar read given Excel 1-based coordinates.
    fn record_cell_1based(&self, sheet_name: &str, row: u32, col: u32) {
        if row == 0 || col == 0 {
            return;
        }
        if let Some(sid) = self.engine.sheet_id(sheet_name) {
            self.collector.record_scalar_with_origin(
                sid,
                row - 1,
                col - 1,
                LiveEdgeOrigin::DirectCell,
            );
        }
    }

    /// Record the resolved rect of a `RangeView`. View bounds are absolute,
    /// 0-based and inclusive. Owned/temporary views (sheet `"__tmp"`) have no
    /// registered `SheetId` and are skipped.
    fn record_view(&self, view: &RangeView<'_>, origin: LiveEdgeOrigin) {
        if view.is_empty() {
            return;
        }
        if let Some(sid) = self.engine.sheet_id(view.sheet_name()) {
            self.collector.record_rect_with_origin(
                sid,
                view.start_row() as u32,
                view.start_col() as u32,
                view.end_row() as u32,
                view.end_col() as u32,
                origin,
            );
        }
    }
}

impl<'a, R: EvaluationContext> ReferenceResolver for RecordingContext<'a, R> {
    fn resolve_cell_reference(
        &self,
        sheet: Option<&str>,
        row: u32,
        col: u32,
    ) -> Result<LiteralValue, ExcelError> {
        // Unqualified (`None`) references are rejected by the engine itself
        // (no current-sheet context at this trait level), so there is nothing
        // attributable to record in that case.
        if let Some(sheet_name) = sheet {
            self.record_cell_1based(sheet_name, row, col);
        }
        self.engine.resolve_cell_reference(sheet, row, col)
    }
}

impl<'a, R: EvaluationContext> RangeResolver for RecordingContext<'a, R> {
    fn resolve_range_reference(
        &self,
        sheet: Option<&str>,
        sr: Option<u32>,
        sc: Option<u32>,
        er: Option<u32>,
        ec: Option<u32>,
    ) -> Result<Box<dyn Range>, ExcelError> {
        // Resolve the rect through the engine's own bound normalisation
        // (used-region for unbounded axes) rather than duplicating it here.
        if let Some(sheet_name) = sheet {
            let reference = ReferenceType::Range {
                sheet: Some(sheet_name.to_string()),
                start_row: sr,
                start_col: sc,
                end_row: er,
                end_col: ec,
                start_row_abs: true,
                start_col_abs: true,
                end_row_abs: true,
                end_col_abs: true,
            };
            if let Ok(view) = self.engine.resolve_range_view(&reference, sheet_name) {
                self.record_view(&view, Self::origin_for_reference(&reference));
            }
        }
        self.engine.resolve_range_reference(sheet, sr, sc, er, ec)
    }
}

impl<'a, R: EvaluationContext> NamedRangeResolver for RecordingContext<'a, R> {
    fn resolve_named_range_reference(
        &self,
        name: &str,
    ) -> Result<Vec<Vec<LiteralValue>>, ExcelError> {
        // Values-only API without sheet/region context; record the *name*
        // member edge (if the name itself is an SCC member) — region-level
        // reads flow through `resolve_range_view` instead.
        self.record_name(name);
        self.engine.resolve_named_range_reference(name)
    }
}

impl<'a, R: EvaluationContext> TableResolver for RecordingContext<'a, R> {
    fn resolve_table_reference(&self, tref: &TableReference) -> Result<Box<dyn Table>, ExcelError> {
        // Opaque `Table` without region context; engine-registered tables are
        // intercepted in `resolve_range_view` instead.
        self.engine.resolve_table_reference(tref)
    }
}

impl<'a, R: EvaluationContext> SourceResolver for RecordingContext<'a, R> {
    fn source_scalar_version(&self, name: &str) -> Option<u64> {
        self.engine.source_scalar_version(name)
    }
    fn resolve_source_scalar(&self, name: &str) -> Result<LiteralValue, ExcelError> {
        self.engine.resolve_source_scalar(name)
    }
    fn source_table_version(&self, name: &str) -> Option<u64> {
        self.engine.source_table_version(name)
    }
    fn resolve_source_table(&self, name: &str) -> Result<Box<dyn Table>, ExcelError> {
        self.engine.resolve_source_table(name)
    }
}

impl<'a, R: EvaluationContext> Resolver for RecordingContext<'a, R> {}

impl<'a, R: EvaluationContext> FunctionProvider for RecordingContext<'a, R> {
    fn planning_semantic_revision(&self) -> Option<u64> {
        self.engine.planning_semantic_revision()
    }

    fn get_function(
        &self,
        ns: &str,
        name: &str,
    ) -> Option<std::sync::Arc<dyn crate::traits::Function>> {
        self.engine.get_function(ns, name)
    }

    fn get_function_for_planning(
        &self,
        ns: &str,
        name: &str,
    ) -> Option<std::sync::Arc<dyn crate::traits::Function>> {
        self.engine.get_function_for_planning(ns, name)
    }
}

impl<'a, R: EvaluationContext> EvaluationContext for RecordingContext<'a, R> {
    /* ── intercept-and-record ── */

    fn resolve_range_view<'c>(
        &'c self,
        reference: &ReferenceType,
        current_sheet: &str,
    ) -> Result<RangeView<'c>, ExcelError> {
        // Named reads can target a name *vertex* that is itself an SCC member
        // (a named formula, spec §7.13). Those resolve to owned-row views with
        // no sheet rect, so they must be recorded by name here in addition to
        // the rect recording below (which covers Cell/Range definitions).
        if let ReferenceType::NamedRange(name) = reference {
            self.record_name(name);
        }
        let view = self.engine.resolve_range_view(reference, current_sheet)?;
        self.record_view(&view, Self::origin_for_reference(reference));
        Ok(view)
    }

    fn record_selected_reference(&self, reference: &ReferenceType, current_sheet: &str) {
        self.engine
            .record_selected_reference(reference, current_sheet)
    }

    fn record_reference_observation(&self, observation: &crate::traits::ReferenceObservation) {
        self.engine.record_reference_observation(observation)
    }

    fn resolve_cell_reference_value(
        &self,
        sheet: Option<&str>,
        row: u32,
        col: u32,
        current_sheet: &str,
    ) -> Result<LiteralValue, ExcelError> {
        self.record_cell_1based(sheet.unwrap_or(current_sheet), row, col);
        self.engine
            .resolve_cell_reference_value(sheet, row, col, current_sheet)
    }

    fn resolve_cell_format(
        &self,
        sheet: Option<&str>,
        row: u32,
        col: u32,
        current_sheet: &str,
    ) -> Option<crate::format::FormatId> {
        self.engine
            .resolve_cell_format(sheet, row, col, current_sheet)
    }

    fn format_class(
        &self,
        format: crate::format::FormatId,
    ) -> Option<formualizer_common::numfmt::FormatClass> {
        self.engine.format_class(format)
    }

    fn record_cell_derived_format(
        &self,
        sheet: &str,
        row: u32,
        col: u32,
        format: Option<crate::format::FormatId>,
    ) {
        self.engine
            .record_cell_derived_format(sheet, row, col, format)
    }

    /* ── pure delegation ── */

    fn thread_pool(&self) -> Option<&std::sync::Arc<rayon::ThreadPool>> {
        self.engine.thread_pool()
    }
    fn cancellation_token(&self) -> Option<crate::engine::CancelToken> {
        self.engine.cancellation_token()
    }
    fn chunk_hint(&self) -> Option<usize> {
        self.engine.chunk_hint()
    }
    fn locale(&self) -> crate::locale::Locale {
        self.engine.locale()
    }
    fn workbook_sheet_count(&self) -> Option<usize> {
        self.engine.workbook_sheet_count()
    }
    fn sheet_index_by_name(&self, sheet: &str) -> Option<usize> {
        self.engine.sheet_index_by_name(sheet)
    }
    fn current_sheet_index(&self, current_sheet: &str) -> Option<usize> {
        self.engine.current_sheet_index(current_sheet)
    }
    fn inspect_reference(
        &self,
        reference: &ReferenceType,
        current_sheet: &str,
    ) -> Result<Option<ReferenceInfo>, ExcelError> {
        self.engine.inspect_reference(reference, current_sheet)
    }
    fn formula_text_at_cell(&self, cell: CellRef) -> Result<Option<String>, ExcelError> {
        self.engine.formula_text_at_cell(cell)
    }
    fn clock(&self) -> &dyn crate::timezone::ClockProvider {
        self.engine.clock()
    }
    fn timezone(&self) -> &crate::timezone::TimeZoneSpec {
        self.engine.timezone()
    }
    fn volatile_level(&self) -> crate::traits::VolatileLevel {
        self.engine.volatile_level()
    }
    fn workbook_seed(&self) -> u64 {
        self.engine.workbook_seed()
    }
    fn recalc_epoch(&self) -> u64 {
        self.engine.recalc_epoch()
    }
    fn used_rows_for_columns(
        &self,
        sheet: &str,
        start_col: u32,
        end_col: u32,
    ) -> Option<(u32, u32)> {
        self.engine.used_rows_for_columns(sheet, start_col, end_col)
    }
    fn used_cols_for_rows(&self, sheet: &str, start_row: u32, end_row: u32) -> Option<(u32, u32)> {
        self.engine.used_cols_for_rows(sheet, start_row, end_row)
    }
    fn sheet_bounds(&self, sheet: &str) -> Option<(u32, u32)> {
        self.engine.sheet_bounds(sheet)
    }
    fn data_snapshot_id(&self) -> u64 {
        self.engine.data_snapshot_id()
    }
    fn reference_generation(
        &self,
        reference: &ReferenceType,
        current_sheet: &str,
    ) -> Option<crate::traits::ReferenceGeneration> {
        self.engine.reference_generation(reference, current_sheet)
    }
    fn backend_caps(&self) -> crate::traits::BackendCaps {
        self.engine.backend_caps()
    }
    fn date_system(&self) -> crate::engine::DateSystem {
        self.engine.date_system()
    }
    fn build_lookup_index(
        &self,
        view: &RangeView<'_>,
        axis: crate::engine::lookup_index_cache::LookupAxis,
    ) -> Option<std::sync::Arc<crate::engine::lookup_index_cache::LookupIndex>> {
        self.engine.build_lookup_index(view, axis)
    }
    fn build_criteria_mask(
        &self,
        view: &RangeView<'_>,
        col_in_view: usize,
        pred: &crate::args::CriteriaPredicate,
    ) -> Option<std::sync::Arc<arrow_array::BooleanArray>> {
        self.engine.build_criteria_mask(view, col_in_view, pred)
    }
    fn build_row_visibility_mask(
        &self,
        view: &RangeView<'_>,
        mode: crate::engine::row_visibility::VisibilityMaskMode,
    ) -> Option<std::sync::Arc<arrow_array::BooleanArray>> {
        self.engine.build_row_visibility_mask(view, mode)
    }
}
