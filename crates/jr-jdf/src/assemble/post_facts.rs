//! What the boarding-post estimator reads of the road compiler's facts and the timetable
//! does not carry (`docs/rules/posts.md`): the observed positions that may be posts of a stop,
//! and for each trip the line version it belongs to. (Skeleton: the estimator's package fills
//! this in.)

/// See the module.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PostFacts {}
