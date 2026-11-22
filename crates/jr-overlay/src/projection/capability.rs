//! Capabilities and their minimum tiers (OVL-R077).

use jr_model::Rule;

use crate::matching::Binding;
use crate::prepare::Prepared;
use crate::profile::Capability;

/// A source may state only what its profile grants: a capability it lists, for a binding of a
/// trip tier at least as strong as the capability's minimum (none: any tier). Everything else
/// stays with the base. Tier names are validated with the profile (B18).
///
/// "Enabled capabilities; everything else is inherited from the base."
pub const OVL_R077: Rule = Rule {
    id: "OVL-R077",
    summary: "a source states only what its profile grants, at the capability's minimum tier",
};

pub const RULES: &[Rule] = &[OVL_R077];

/// Whether a source's profile lists the capability at all.
pub fn enabled(prepared: &Prepared<'_>, source: u32, capability: Capability) -> bool {
    prepared.sources[source as usize]
        .profile
        .capabilities
        .contains_key(&capability)
}

/// Whether a binding may state the capability.
pub fn eligible(prepared: &Prepared<'_>, binding: &Binding, capability: Capability) -> bool {
    prepared.sources[binding.source as usize]
        .profile
        .grants(capability, binding.candidate.tier)
}
