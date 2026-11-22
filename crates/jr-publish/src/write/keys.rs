//! `source_key` and `call_key`. A namespace the contract does not declare, or a key whose
//! entity is not of the namespace's kind, is an error.

use jr_model::timetable::{BindingMethod, Entity, Namespace};

use super::PublishError;
use super::assemble::{self, date, int32, text};
use super::scope::Published;
use crate::contract::contract;
use crate::table::Table;

fn declared(namespace: &Namespace) -> Result<&'static str, PublishError> {
    contract()
        .namespaces
        .get(namespace.as_str())
        .map(|spec| spec.entity_kind.as_str())
        .ok_or_else(|| PublishError::UndeclaredNamespace(namespace.as_str().to_owned()))
}

fn binding(method: BindingMethod) -> &'static str {
    match method {
        BindingMethod::Identity => "identity",
        BindingMethod::SourceNative => "source_native",
        BindingMethod::AuthoritativeSourceTripSet => "authoritative_source_trip_set",
        BindingMethod::StructuralMatch => "structural_match",
        BindingMethod::CompanionAssertion => "companion_assertion",
        BindingMethod::OperatorCrosswalk => "operator_crosswalk",
    }
}

pub fn source_key(p: &Published) -> Result<Table, PublishError> {
    let t = p.t;
    let rows: Vec<_> = p.keys.iter().map(|&index| &t.keys[index]).collect();
    let mut kinds = Vec::with_capacity(rows.len());
    let mut public_ids = Vec::with_capacity(rows.len());
    for key in &rows {
        let (kind, id) = match key.entity {
            Entity::Trip(id) => ("trip", t.trips[id].id.as_str()),
            Entity::Route(id) => ("route", t.routes[id].id.as_str()),
            Entity::Place(id) => ("location", t.places[id].id.as_str()),
            Entity::BoardingPoint(id) => ("location", t.boarding_points[id].id.as_str()),
        };
        let expected = declared(&key.namespace)?;
        if expected != kind {
            return Err(PublishError::EntityKind {
                namespace: key.namespace.as_str().to_owned(),
                identifier: key.identifier.to_string(),
                entity: kind,
                expected: expected.to_owned(),
            });
        }
        kinds.push(kind);
        public_ids.push(id);
    }
    assemble::finish(
        "source_key",
        vec![
            ("entity_kind", text(kinds.iter().map(|k| Some(*k)))),
            (
                "namespace",
                text(rows.iter().map(|k| Some(k.namespace.as_str()))),
            ),
            (
                "identifier",
                text(rows.iter().map(|k| Some(&*k.identifier))),
            ),
            ("public_id", text(public_ids.iter().map(|id| Some(*id)))),
            ("valid_from", date(rows.iter().map(|k| Some(k.valid_from)))),
            ("valid_to", date(rows.iter().map(|k| Some(k.valid_to)))),
            (
                "binding_method",
                text(rows.iter().map(|k| Some(binding(k.binding)))),
            ),
        ],
    )
}

pub fn call_key(p: &Published) -> Result<Table, PublishError> {
    let t = p.t;
    let rows: Vec<_> = p
        .call_keys
        .iter()
        .map(|&index| &t.call_keys[index])
        .collect();
    for key in &rows {
        declared(&key.namespace)?;
    }
    assemble::finish(
        "call_key",
        vec![
            (
                "namespace",
                text(rows.iter().map(|k| Some(k.namespace.as_str()))),
            ),
            (
                "identifier",
                text(rows.iter().map(|k| Some(&*k.identifier))),
            ),
            (
                "source_sequence",
                text(rows.iter().map(|k| Some(&*k.source_sequence))),
            ),
            (
                "trip_id",
                text(rows.iter().map(|k| Some(t.trips[k.trip].id.as_str()))),
            ),
            ("sequence", int32(rows.iter().map(|k| Some(k.sequence)))),
        ],
    )
}
