//! Application order of the source messages (R002, R005, R006, R010, R011, R012).

use std::collections::{HashMap, HashSet};

use jiff::tz::{Offset, TimeZone};
use jr_formats::czptt::{CancellationMessage, Identifier, Message, ObjectType, TimetableMessage};
use jr_model::identity::czptt::transport_identifier;
use jr_model::{Diagnostic, Diagnostics, Rule, Severity};

use crate::policy::Policy;
use crate::{CompileError, SourceMessage};

/// SŽ writes `CZPTTCreation` and `CZPTTCancelation` without an offset (every message of the
/// golden subset); they are Czech local times and are ordered as instants.
pub(crate) const R002: Rule = Rule {
    id: "CZPTT-R002",
    summary: "A message's event time without an offset is local time in the policy zone",
};

/// Cancellations are published as separate messages; applied in the wrong order they would
/// resurrect a cancelled day or cancel a later timetable. The annual archive is the base.
pub(crate) const R005: Rule = Rule {
    id: "CZPTT-R005",
    summary: "Annual messages first, then changes by event time, timetables before cancellations",
};

/// The same change object is published more than once (monthly archives overlap).
pub(crate) const R006: Rule = Rule {
    id: "CZPTT-R006",
    summary: "An exact duplicate payload is dropped; the first in application order is kept",
};

/// The reader rejects a message it cannot read; one bad message must not stop the build.
pub(crate) const R010: Rule = Rule {
    id: "CZPTT-R010",
    summary: "A message the reader cannot read is diagnosed and skipped",
};

/// A path is known by its PA identifier; without one a message cannot be merged.
pub(crate) const R011: Rule = Rule {
    id: "CZPTT-R011",
    summary: "A message is identified by its first PA identifier, published as Pa:<…>",
};

/// One identity per path (replaces R003 and R007): a PA published twice with different content
/// leaves no way to tell which one is meant, so the build stops, as it did before.
pub(crate) const R012: Rule = Rule {
    id: "CZPTT-R012",
    summary: "Two different timetable payloads for one PA stop the build",
};

pub(crate) enum Payload {
    Timetable(TimetableMessage),
    Cancellation(CancellationMessage),
}

/// A message in application order, with its path identity.
pub(crate) struct Ordered {
    pub source_path: Box<str>,
    pub pa: Box<str>,
    pub payload: Payload,
}

/// The published form of the first identifier of `object_type` (R011).
pub(crate) fn identifier_text<'a>(
    identifiers: &'a [Identifier],
    object_type: ObjectType,
    policy: &Policy,
) -> Option<(Box<str>, &'a Identifier)> {
    let identifier = identifiers
        .iter()
        .find(|identifier| identifier.object_type == object_type)?;
    let kind = match object_type {
        ObjectType::Pa => &policy.identifiers.path_object_type,
        ObjectType::Tr => &policy.identifiers.train_object_type,
    };
    let text = transport_identifier(
        kind,
        &identifier.company,
        &identifier.core,
        &identifier.variant,
        &identifier.timetable_year,
    );
    Some((text.into_boxed_str(), identifier))
}

struct Pending {
    source_path: Box<str>,
    annual: bool,
    digest: [u8; 32],
    event: jiff::Timestamp,
    message: Message,
}

pub(crate) fn order(
    messages: Vec<SourceMessage>,
    policy: &Policy,
    diagnostics: &mut Diagnostics,
) -> Result<Vec<Ordered>, CompileError> {
    let zone_name = &policy.ordering.naive_timestamp_zone;
    let zone = TimeZone::get(zone_name).map_err(|error| CompileError::TimeZone {
        zone: zone_name.clone(),
        message: error.to_string(),
    })?;

    let mut pending = Vec::with_capacity(messages.len());
    for source in messages {
        let message = match source.message {
            Ok(message) => message,
            Err(error) => {
                diagnostics.push(
                    Diagnostic::new(
                        "czptt_message_unreadable",
                        Severity::Warning,
                        &*source.source_path,
                        &*error,
                    )
                    .rule(R010),
                );
                continue;
            }
        };
        let stamp = match &message {
            Message::Timetable(message) => message.creation,
            Message::Cancellation(message) => message.cancellation,
        };
        let event = match stamp.offset_seconds {
            Some(seconds) => {
                Offset::from_seconds(seconds).and_then(|offset| offset.to_timestamp(stamp.local))
            }
            None => zone.to_ambiguous_timestamp(stamp.local).compatible(),
        };
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                diagnostics.push(
                    Diagnostic::new(
                        "czptt_event_time_invalid",
                        Severity::Warning,
                        &*source.source_path,
                        format!("{}: {error}", stamp.local),
                    )
                    .rule(R002),
                );
                continue;
            }
        };
        pending.push(Pending {
            source_path: source.source_path,
            annual: source.annual,
            digest: source.digest,
            event,
            message,
        });
    }

    let (mut ordered, mut changes): (Vec<_>, Vec<_>) =
        pending.into_iter().partition(|message| message.annual);
    ordered.sort_by(|a, b| a.source_path.cmp(&b.source_path));
    let rank = |message: &Message| match message {
        Message::Timetable(_) => 0,
        Message::Cancellation(_) => 1,
    };
    changes.sort_by(|a, b| {
        (a.event, rank(&a.message), &a.source_path).cmp(&(
            b.event,
            rank(&b.message),
            &b.source_path,
        ))
    });
    ordered.append(&mut changes);

    // Looked up, never iterated.
    let mut seen: HashSet<[u8; 32]> = HashSet::new();
    let mut timetables: HashMap<Box<str>, ([u8; 32], Box<str>)> = HashMap::new();
    let mut out = Vec::with_capacity(ordered.len());
    for message in ordered {
        if !seen.insert(message.digest) {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_duplicate_message",
                    Severity::Info,
                    &*message.source_path,
                    "an identical payload was applied earlier",
                )
                .rule(R006),
            );
            continue;
        }
        let identifiers = match &message.message {
            Message::Timetable(timetable) => &timetable.identifiers,
            Message::Cancellation(cancellation) => &cancellation.identifiers,
        };
        let Some((pa, _)) = identifier_text(identifiers, ObjectType::Pa, policy) else {
            diagnostics.push(
                Diagnostic::new(
                    "czptt_message_without_path",
                    Severity::Warning,
                    &*message.source_path,
                    "the message has no PA identifier",
                )
                .rule(R011),
            );
            continue;
        };
        let payload = match message.message {
            Message::Timetable(timetable) => {
                match timetables.get(&pa) {
                    Some((digest, first)) if *digest != message.digest => {
                        return Err(CompileError::ConflictingPath {
                            pa,
                            first: first.clone(),
                            second: message.source_path,
                        });
                    }
                    Some(_) => {}
                    None => {
                        timetables
                            .insert(pa.clone(), (message.digest, message.source_path.clone()));
                    }
                }
                Payload::Timetable(timetable)
            }
            Message::Cancellation(cancellation) => Payload::Cancellation(cancellation),
        };
        out.push(Ordered {
            source_path: message.source_path,
            pa,
            payload,
        });
    }
    Ok(out)
}
