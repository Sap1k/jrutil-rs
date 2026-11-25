//! The evaluator's rules (`docs/rules/posts.md`, sections 3.6–3.10 and 12).

use jr_model::Rule;

/// The pack is walked as one ordered join: per context, per variant (rank order), one row per
/// route point of the stop, carrying the variant's relative costs and service-edge count. A
/// variant's failure reason stands in for a missing attachment reason, and any reason makes the
/// row unavailable.
///
/// The three routing relations are written context-major; the evaluator sees a row per
/// (context, variant, route point) exactly as F# `replayScoreRows` produced it.
pub const POST_R062: Rule = Rule {
    id: "POST-R062",
    summary: "one evidence row per (context, variant, route point); variant failure fills in",
};

/// Two route points of a stop are compatible when they are within the consolidation diameter,
/// do not differ in lifecycle where one is obsolete, and over every (context, variant) both
/// have rows for: faces agree when both present, sides are not confidently opposite, headings
/// differ by at most the policy angle and the chainage along the left heading is within the
/// policy distance; and fewer than the policy count of contexts distinguish them by geometry.
///
/// One physical post is often mapped twice (an OSM stop node and a catalogue point a metre
/// away); merging them keeps one post from competing with itself.
pub const POST_R070: Rule = Rule {
    id: "POST-R070",
    summary: "route points merge only when close, lifecycle-consistent and not told apart",
};

/// Clusters start as single points in route-point-id order; each round merges the pairwise
/// compatible pair with the smallest diameter, ties by the joined sorted route point ids; the
/// union takes the earlier place.
///
/// Closest-first merging with a content tie-break makes the hypotheses independent of input
/// order.
pub const POST_R071: Rule = Rule {
    id: "POST-R071",
    summary: "closest compatible pair merges first, ties by joined route point ids",
};

/// A hypothesis is `sha("{stop}|{its distinct observation ids, ordinal, joined |}")` (route
/// point ids when it has no observation); its position is the medoid (least sum of local
/// distances, then route point id); it is current/obsolete if any member is, its support is
/// the members' sum and its modes the members' union in first-seen order.
///
/// The observation-derived identity is what makes decisions comparable across captures and
/// implementations (section 5).
pub const POST_R072: Rule = Rule {
    id: "POST-R072",
    summary: "hypothesis id from stop and observation ids; medoid position; union of facts",
};

/// Rows are aggregated per (context, variant, hypothesis): medians of the present routed
/// excess, corridor distance and offset; a face or heading only when the members agree on a
/// single value (B16 kept); the failure only when every member has one (the smallest).
///
/// A hypothesis stands for all its member points in every context.
pub const POST_R073: Rule = Rule {
    id: "POST-R073",
    summary: "rows per hypothesis: medians, unanimous faces and headings, failure if all fail",
};

/// Per context the selected corridor ranks are rank 0 and every rank below the policy maximum
/// whose relative cost is within the absolute or relative tie. A hypothesis keeps its rank-0
/// row; it is valid when it has an available, face-carrying, failure-free row for every
/// selected rank, and its corridors agree when that face is one; otherwise the row fails with
/// `alternative-corridor-invariant`.
///
/// When two corridors are almost equally cheap the vehicle may take either; a post is only
/// trusted when both corridors put it on the same face.
pub const POST_R080: Rule = Rule {
    id: "POST-R080",
    summary: "near-tied corridors must agree on the face, else alternative-corridor-invariant",
};

/// Rejection, first that applies: obsolete and not current; the hypothesis' effective modes
/// (its own, or those of hypotheses within the inheritance radius) all exclude the context
/// mode; the row's topology failure; corridor distance, routed excess or a confident left side
/// beyond the policy gates.
///
/// Most, nádraží: tram-only stop positions and catalogue points beside them are not bus posts;
/// a catalogue point beside a bus platform is.
pub const POST_R081: Rule = Rule {
    id: "POST-R081",
    summary: "hard gates: lifecycle, inherited modes, topology, distance, excess, left side",
};

/// `geometry = clamp(w_a·alignment + w_s·side + w_p·proximity + w_r·routed fit)`, alignment
/// falling to 0 at 90°, side 1 right / 0.5 neutral / 0 left, proximity to 0 at 50 m, routed fit
/// to 0 at the policy excess.
///
/// Czech traffic drives on the right: a post right of travel, close to the road, aligned with
/// it and without a detour is likely.
pub const POST_R082: Rule = Rule {
    id: "POST-R082",
    summary: "geometry score from alignment, side, proximity and routed fit",
};

/// Source support `min(max, (support − 1) × per weight)` plus modality (conflict for a
/// compatible denied mode, explicit support, estimated support without modes), clamped, applied
/// as `supportingScore`.
///
/// Several sources and an explicit bus tag make a post more credible.
pub const POST_R083: Rule = Rule {
    id: "POST-R083",
    summary: "support and modality adjustments of the row score",
};

/// Per context key (without the authored key, B12 kept) the eligible rows ranked by (final
/// desc, geometry desc, hypothesis id); the first wins with its lead over the runner-up.
///
/// The per-context winners are the votes the family decision counts.
pub const POST_R084: Rule = Rule {
    id: "POST-R084",
    summary: "each context's winner and lead by final score, geometry, hypothesis id",
};

/// Per family, each eligible hypothesis scores the median final and geometry of its rows,
/// raised by popularity (contexts of other families it won, capped) and isolation from the
/// nearest other hypothesis (B18 metric kept).
///
/// A post other lines already use, and one standing apart, is more likely the real one.
pub const POST_R085: Rule = Rule {
    id: "POST-R085",
    summary: "family candidate scores: medians plus popularity and isolation",
};

/// The top candidate is contradicted by a confident dissenting context winner; consensus needs
/// the policy share of the family's contexts; an established post needs enough external
/// support over the runner-up without a large geometry disadvantage.
///
/// Families mix lines and authored and unlabelled calls; a confident dissenter vetoes.
pub const POST_R086: Rule = Rule {
    id: "POST-R086",
    summary: "contradiction, consensus and established-post tests",
};

/// Physical when the final score reaches the policy minimum, the margin (or a material routed
/// advantage without geometry loss) suffices, nothing contradicts, consensus or establishment
/// holds (or too few contexts), and alternative corridors agree.
///
/// The policy is conservative: a guessed post is worse than none.
pub const POST_R087: Rule = Rule {
    id: "POST-R087",
    summary: "physical only above score and margin, with consensus and agreeing corridors",
};

/// One decision per (stop, family): physical with the hypothesis, score, margin and first face,
/// else centroid; ordered by (mode, line, family). `unsafePhysical` is not ported.
///
/// The family decision decides the family's unlabelled contexts under the heuristic scorer and
/// feeds authored positions under both.
pub const POST_R088: Rule = Rule {
    id: "POST-R088",
    summary: "one heuristic decision per movement family",
};

/// `resolution.minimum_plausible_score` is validated and otherwise ignored.
///
/// The document format keeps the key; no decision reads it (F# history).
pub const POST_R089: Rule = Rule {
    id: "POST-R089",
    summary: "minimum_plausible_score validated, never read",
};

/// With a learned scorer, the learned decision replaces the heuristic one for unlabelled
/// contexts only; an unlabelled context the scorer saw no candidate for resolves to the
/// centroid. Authored contexts keep the heuristic family decision.
///
/// The learned scorer is production (Oběhy's `learned-v2.json`); authored posts stay on the
/// heuristic path (verdict Q4).
pub const POST_R100: Rule = Rule {
    id: "POST-R100",
    summary: "learned decisions for unlabelled contexts; authored stay heuristic",
};

/// A source class is stop-level when it occurs at 20 or more stops where OSM has two or more
/// route points and has two or more route points itself at fewer than half of them; computed
/// over the whole pack (verdict Q11).
///
/// A stop-level source publishes one point per stop; the model was trained with this
/// statistic.
pub const POST_R101: Rule = Rule {
    id: "POST-R101",
    summary: "stop-level sources: >= 20 multi-post stops, multi-point at < 50 % of them",
};

/// A hypothesis is a learned choice when some member observation is not from a stop-level
/// source; its `local_ref`, platform flag, sources and modes come from those observations.
///
/// Candidates only stop-level sources know are not posts.
pub const POST_R102: Rule = Rule {
    id: "POST-R102",
    summary: "post-level hypotheses: backed by a non-stop-level source",
};

/// Choices cluster closest-first (complete linkage, at most 25 m) into physical posts that
/// never hold one source twice nor two `local_ref` values; post support, deficit, modes and
/// bays (three or more numbered platforms at the stop) follow.
///
/// A source maps each post once; differing references are different posts.
pub const POST_R103: Rule = Rule {
    id: "POST-R103",
    summary: "physical posts: <= 25 m, one source once, at most one local_ref",
};

/// Posts (in post-id string order) cluster into areas at most 40 m wide that never mix tram
/// and road posts.
///
/// When the post is uncertain the area may still be clear (verdict Q6).
pub const POST_R104: Rule = Rule {
    id: "POST-R104",
    summary: "areas: posts within 40 m, never tram and road together",
};

/// Candidates are the rank-0 consolidated rows of post-level hypotheses whose post modes are
/// empty or compatible with the context mode; gated rows stay with `eligible = 0`.
///
/// Modes propagate through the physical post.
pub const POST_R105: Rule = Rule {
    id: "POST-R105",
    summary: "learned candidates: rank-0 rows of post-level hypotheses with usable post modes",
};

/// The seventeen frozen features computed by name (section 2.5).
///
/// The model was trained on exactly these; any other name is refused.
pub const POST_R106: Rule = Rule {
    id: "POST-R106",
    summary: "frozen learned features by name",
};

/// Stage 1 softmax per context with a null option, post masses, the six collective features
/// (reverse, same movement, same next, same previous), stage 2 softmax.
///
/// Related contexts at a stop inform each other: the reverse movement uses the other post,
/// lines making the same movement share one.
pub const POST_R107: Rule = Rule {
    id: "POST-R107",
    summary: "two-stage conditional logit with collective features",
};

/// Physical when the top candidate's post mass reaches the simple or (three or more posts)
/// complex threshold; else area when an area threshold exists and is reached; else abstain.
///
/// Thresholds are calibrated per stratum to 0.9 precision.
pub const POST_R108: Rule = Rule {
    id: "POST-R108",
    summary: "physical above the threshold, else area, else abstain",
};

/// Physical and Area decisions both publish the chosen hypothesis (verdict Q6); the recorded
/// score is the post probability, the post and area probabilities are reported in their own
/// columns, and a learned decision records no margin (B10 fixed: F# wrote the area probability
/// as the margin). An Area decision's post is published `approximate` (POST-R130).
pub const POST_R109: Rule = Rule {
    id: "POST-R109",
    summary: "learned physical and area publish the hypothesis; probabilities reported as such",
};

/// One decision per context: authored contexts carry their family's heuristic resolution and
/// never a location; unlabelled ones the learned or heuristic decision.
///
/// Only unlabelled calls receive an estimated post; an authored post is the carrier's word.
pub const POST_R120: Rule = Rule {
    id: "POST-R120",
    summary: "a decision per context; authored contexts never get a location",
};

/// A published estimated location is identified by its stop and hypothesis.
///
/// It is the input of the ordinal (R131, R132); the `estimated:` hash is not built.
pub const POST_R121: Rule = Rule {
    id: "POST-R121",
    summary: "estimated location = (stop, hypothesis)",
};

/// On request (`[jdf.posts] training`), the learned scorer's rows are reported as they were
/// scored (`post_scorer_rows`: context, stop, hypothesis, post, area, movement, position and
/// the stage-1 features) with the published calls of each context (`post_context_calls`): the
/// scorer's training table comes from the estimator that uses it (verdict Q14). Nothing is
/// decided differently.
pub const POST_R141: Rule = Rule {
    id: "POST-R141",
    summary: "the learned scorer's rows and each context's calls are reported for training",
};

/// An authored post takes a position when its contexts' family decisions name exactly one
/// hypothesis, unanimously if the policy says so, each above the authored score and margin.
/// Whether an unrelated unlabelled context at the stop published that hypothesis does not
/// matter: at a stop where every call is authored, the posts could never be placed (B11 fixed,
/// `docs/roadmap.md`).
///
/// Strong agreement lets the carrier's post take the estimated position.
pub const POST_R122: Rule = Rule {
    id: "POST-R122",
    summary: "authored posts positioned by unanimous strong family decisions",
};

/// Counts per rule replace the F# counters (contexts, candidate-bearing stops, unresolved
/// unlabelled contexts, positioned authored posts).
///
/// The manifest and the diagnostics report them.
pub const POST_R123: Rule = Rule {
    id: "POST-R123",
    summary: "evaluation counts",
};

/// Evaluation runs per stop in parallel; results are put back in stop order, so decisions do
/// not depend on the thread count.
///
/// Stops are independent given the pack-wide stop-level sources (AGENTS.md rule 7).
pub const POST_R180: Rule = Rule {
    id: "POST-R180",
    summary: "per-stop evaluation in parallel, results in stop order",
};

/// The policy document's unknown keys are an error; F# ignored them.
///
/// A misspelt threshold must not silently fall back to zero.
pub const POST_R181: Rule = Rule {
    id: "POST-R181",
    summary: "unknown policy document keys are rejected",
};

/// Every rule of the evaluator and of the pack it reads.
pub const RULES: &[Rule] = &[
    jr_formats::post_evidence::POST_R060,
    jr_formats::post_evidence::POST_R061,
    POST_R062,
    POST_R070,
    POST_R071,
    POST_R072,
    POST_R073,
    POST_R080,
    POST_R081,
    POST_R082,
    POST_R083,
    POST_R084,
    POST_R085,
    POST_R086,
    POST_R087,
    POST_R088,
    POST_R089,
    POST_R100,
    POST_R101,
    POST_R102,
    POST_R103,
    POST_R104,
    POST_R105,
    POST_R106,
    POST_R107,
    POST_R108,
    POST_R109,
    POST_R120,
    POST_R121,
    POST_R122,
    POST_R141,
    POST_R123,
    POST_R180,
    POST_R181,
    jr_formats::post_evidence::POST_R182,
];
