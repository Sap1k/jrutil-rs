# post-scorer

Training and judging of the learned stop-post scorer that `jr-posts` runs
(`crates/jr-posts/src/evaluate/scorer.rs`) when the post policy carries a model. The model is
trained where posts are known (PID, IDS JMK) and runs everywhere. The shipped model is Oběhy's
`src/obehy/data/post-inference/learned-v2.json`. Moved here from the F# JrUtil's
`scripts/post-scorer` (2026-11-25), whose feature export and labelling chain the Rust training
export replaces.

## Data flow

| Step | Tool | Output |
|---|---|---|
| Capture, labelled regions' catalogues excluded | a jrutil build with `[jdf.posts] training = true` and `reports = …` (POST-R141) | `post_scorer_rows.csv`, `post_context_calls.csv`, `post_decisions.csv` |
| Labels | a road package with the regional feeds overlaid, on a base without estimated posts | the real post of every bound call |
| Candidate table | `uv run python training_table.py <reports> <overlay package> <candidates.parquet>` | one row per (context, candidate), labelled, with the shipped model's decision as the baseline |
| Train and judge | `uv run python train_scorer.py <candidates.parquet> <shipped policy.json> <output policy.json>` | the new policy, only when it beats the shipped one on held-out stops |
| Rust parity fixture | `uv run python -m post_scorer.golden --candidates … --model … --output …` | `crates/jr-posts/tests/data/post-scorer-golden.json` |

## Labels
- A context's real post is the majority regional post of its bound calls; contexts where the
  posts disagree (share under 0.8) are unlabelled.
- Every candidate within 20 m of the real post is correct, because one physical post is often
  mapped more than once (OSM platform, stop position, catalogue points).
- If no candidate lies within 35 m, the label is "no correct candidate".
- Contexts whose nearest candidate falls between 20 and 35 m are dropped as ambiguous.
- Posts more than 300 m from every candidate are treated as alignment errors and dropped.

## Leakage guards
- The regional catalogues (`PID.csv`, `MPVNet_PID.csv`, `IDSJMK.csv`) are left out of the
  capture. They contain every bay of that region.
- Features never include source identity (catalogue, source kind, source-support weight) or
  network-specific OSM references such as `ref:PID`.
- The train/test split is by stop (`stable_fold`, a hash into five folds; fold 0 is the test),
  so the contexts of one terminal never straddle it.

## Physical posts
`posts.py` groups each stop's candidates into posts by closest-first complete linkage: a post
never contains the same source twice, its diameter stays within 25 m, and differing `local_ref`
values never merge. Two OSM bays 4 m apart stay separate, while four sources spread over 17 m
merge. A post's probability mass is the sum over its duplicates.

## Model
A conditional logit: a softmax over each context's candidates plus a "no correct candidate"
alternative with a learned constant utility. Training maximises the probability mass on the
correct set, which can contain several candidates. Features are standardised; coefficients and
scaling are exported in the model JSON for the Rust scorer.

**Two stages.**
- Stage 1 scores each context alone.
- Stage 2 adds what related contexts at the same stop believe, from stage-1 probabilities
  (`collective.py`): `reverse_mass` (how strongly the reverse movement favours this post) and
  `same_movement_mass` (agreement among other lines making the same movement).
- Stage 2 is fitted on out-of-fold stage-1 scores (folds by stop).

**Decisions.**
- A post is published when its probability mass reaches the threshold of its stratum: simple
  stops (up to 2 posts) or complex stops (3 or more).
- Otherwise, when the area (posts within 40 m, never mixing tram and road posts) holding most of
  the mass reaches the area threshold, the area's most probable post is published (`Area`).
- Otherwise the context abstains.
- `train_scorer.py` calibrates the thresholds on out-of-fold scores of the training folds, per
  stratum, to the shipped model's call precision there, and judges both models on fold 0 per
  call.

**Frozen features** (the Rust scorer computes exactly these, in this order; `posts.py`
`FEATURE_COLUMNS`): `side`, `alignment`, `proximity`, `corridor_distance`, `routed_fit`,
`routed_excess_metres_capped`, `excess_minus_context_min`, `eligible`, `has_local_ref`,
`is_bay`, `post_support`, `post_support_deficit`, `is_tram`, `is_trolleybus`,
`anchor_previous_missing`, `anchor_next_missing`, `service_edge_count`; stage 2 adds the six
collective columns. Changing them needs a new model format and a Rust change.

The **street-over-bay** metric needs no labels: it counts publications of a non-bay candidate
at a stop that has bays.

## Checks
```bash
uv run --group dev pytest -q
```
