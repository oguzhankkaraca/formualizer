#![forbid(unsafe_code)]

mod evaluator;
mod model;
mod shadow;

pub use evaluator::{EvaluationHost, PocEngine, PocModelStats, ReferenceResolver, ScheduleReport};
pub use model::{
    CellId, CellState, DependencyDescriptor, EffectKey, EvaluationResult, ExecutionRead,
    FormulaReadTrace, FormulaRecord, InvalidationDependency, NameDefinition, NameDefinitionRecord,
    NameId, NameRegistry, NameScope, PocValue, RangeDescriptor, ReadRecorder, ReferenceValue,
    ResolvedKind, SpillRef, TableDescriptor, TraceReport,
};
pub use shadow::{
    ArtifactShadowReport, HeavyWitnessAudit, RealHeavyPocReport, RealSequenceStep, ShadowMetrics,
    ShadowModel, ShadowRelation, WitnessCellAudit, WitnessEdgeAudit, XlsxPocModel,
    build_artifact_shadow_report, build_xlsx_shadow_metrics, build_xlsx_shadow_pair_report,
    load_xlsx_poc_model, run_real_heavy_poc, run_real_heavy_witness_audit, run_real_light_poc,
};

#[cfg(test)]
mod tests;
