//! The serving contract (`contracts/serving-v5.json`), parsed once from the embedded JSON.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// The normative serving contract this crate reads and writes.
const SERVING_CONTRACT: &str = include_str!("../../../contracts/serving-v5.json");

/// Bundle format, version and companion versions from `contracts/production-v3.json`.
pub const BUNDLE_FORMAT: &str = "jrutil-production";
pub const BUNDLE_VERSION: i64 = 3;
pub const DIAGNOSTICS_SCHEMA_VERSION: i64 = 1;
pub const IDENTITY_CONTRACT: &str = "jrutil-identity-v1";

/// Feed prefixes every primary-key identifier starts with (the contract's `ids` rule).
pub const FEED_PREFIXES: [&str; 3] = ["jdf:", "overlay:", "czptt:"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    pub major: u16,
    pub minor: u16,
    /// Contract order.
    pub relations: Vec<RelationSpec>,
    /// Enumeration name -> allowed values (int16 enumerations as decimal text).
    pub enumerations: BTreeMap<String, Vec<String>>,
    pub namespaces: BTreeMap<String, NamespaceSpec>,
    /// Parquet key-value metadata every relation file carries; `<relation name>` stands for
    /// the relation's own name.
    pub parquet_metadata: BTreeMap<String, String>,
    /// The contract's largest allowed Parquet row group.
    pub row_group_maximum: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationSpec {
    pub name: String,
    pub fields: Vec<FieldSpec>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpec {
    pub name: String,
    pub ty: FieldType,
    pub nullable: bool,
    pub enumeration: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FieldType {
    String,
    Int16,
    Int32,
    Double,
    Bool,
    Date32,
}

impl FieldType {
    /// The contract's spelling (`string`, `int16`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            FieldType::String => "string",
            FieldType::Int16 => "int16",
            FieldType::Int32 => "int32",
            FieldType::Double => "double",
            FieldType::Bool => "bool",
            FieldType::Date32 => "date32",
        }
    }

    pub fn parse(text: &str) -> Option<FieldType> {
        Some(match text {
            "string" => FieldType::String,
            "int16" => FieldType::Int16,
            "int32" => FieldType::Int32,
            "double" => FieldType::Double,
            "bool" => FieldType::Bool,
            "date32" => FieldType::Date32,
            _ => return None,
        })
    }
}

impl std::fmt::Display for FieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ForeignKey {
    pub fields: Vec<String>,
    pub relation: String,
    pub target_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceSpec {
    pub entity_kind: String,
    /// Prose description of the identifier encoding.
    pub encoding: String,
}

/// The embedded serving contract.
pub fn contract() -> &'static Contract {
    static CONTRACT: OnceLock<Contract> = OnceLock::new();
    CONTRACT.get_or_init(|| match parse(SERVING_CONTRACT) {
        Ok(contract) => contract,
        // The contract is compiled in; the unit tests prove it parses.
        Err(error) => panic!("embedded serving contract is invalid: {error}"),
    })
}

impl Contract {
    pub fn relation(&self, name: &str) -> Option<&RelationSpec> {
        self.relations.iter().find(|relation| relation.name == name)
    }

    /// `major.minor`.
    pub fn version(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

impl RelationSpec {
    pub fn field(&self, name: &str) -> Option<&FieldSpec> {
        self.fields.iter().find(|field| field.name == name)
    }
}

/// `(major, minor)` of a `major.minor` version string.
pub fn parse_version(text: &str) -> Option<(u16, u16)> {
    let (major, minor) = text.split_once('.')?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !digits(major) || !digits(minor) {
        return None;
    }
    Some((major.parse().ok()?, minor.parse().ok()?))
}

#[derive(Deserialize)]
struct RawContract {
    serving_schema_version: String,
    parquet_metadata: BTreeMap<String, String>,
    rules: RawRules,
    enumerations: BTreeMap<String, RawEnumeration>,
    namespaces: BTreeMap<String, RawNamespace>,
    relations: Vec<RawRelation>,
}

#[derive(Deserialize)]
struct RawRules {
    row_group_maximum: u64,
}

#[derive(Deserialize)]
struct RawEnumeration {
    values: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct RawNamespace {
    entity_kind: String,
    encoding: String,
}

#[derive(Deserialize)]
struct RawRelation {
    name: String,
    fields: Vec<RawField>,
    primary_key: Vec<String>,
    foreign_keys: Vec<ForeignKey>,
}

#[derive(Deserialize)]
struct RawField {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    nullable: bool,
    #[serde(rename = "enum")]
    enumeration: Option<String>,
}

fn parse(text: &str) -> Result<Contract, String> {
    let raw: RawContract = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let (major, minor) = parse_version(&raw.serving_schema_version)
        .ok_or_else(|| format!("bad version {}", raw.serving_schema_version))?;
    let enumerations: BTreeMap<String, Vec<String>> = raw
        .enumerations
        .into_iter()
        .map(|(name, value)| (name, value.values.keys().cloned().collect()))
        .collect();
    let mut relations = Vec::with_capacity(raw.relations.len());
    for relation in raw.relations {
        let mut fields = Vec::with_capacity(relation.fields.len());
        for field in relation.fields {
            let ty = FieldType::parse(&field.ty).ok_or_else(|| {
                format!(
                    "{}.{}: unknown type {}",
                    relation.name, field.name, field.ty
                )
            })?;
            if let Some(name) = &field.enumeration
                && !enumerations.contains_key(name)
            {
                return Err(format!(
                    "{}.{}: unknown enum {name}",
                    relation.name, field.name
                ));
            }
            fields.push(FieldSpec {
                name: field.name,
                ty,
                nullable: field.nullable,
                enumeration: field.enumeration,
            });
        }
        relations.push(RelationSpec {
            name: relation.name,
            fields,
            primary_key: relation.primary_key,
            foreign_keys: relation.foreign_keys,
        });
    }
    let namespaces = raw
        .namespaces
        .into_iter()
        .map(|(name, value)| {
            (
                name,
                NamespaceSpec {
                    entity_kind: value.entity_kind,
                    encoding: value.encoding,
                },
            )
        })
        .collect();
    Ok(Contract {
        major,
        minor,
        relations,
        enumerations,
        namespaces,
        parquet_metadata: raw.parquet_metadata,
        row_group_maximum: raw.rules.row_group_maximum,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_contract_loads_with_every_relation_and_key() {
        let contract = contract();
        assert_eq!((contract.major, contract.minor), (5, 2));
        assert_eq!(contract.relations.len(), 19);
        assert_eq!(contract.relations[0].name, "agency");

        let call_zone = contract.relation("call_zone").expect("call_zone");
        assert_eq!(
            call_zone.primary_key,
            ["trip_id", "sequence", "source_order"]
        );
        assert_eq!(
            call_zone.foreign_keys,
            [ForeignKey {
                fields: vec!["trip_id".into(), "sequence".into()],
                relation: "trip_call".into(),
                target_fields: vec!["trip_id".into(), "sequence".into()],
            }]
        );
        let pickup = contract
            .relation("trip_call")
            .and_then(|r| r.field("pickup_type"));
        assert_eq!(
            pickup.map(|f| (f.ty, f.nullable)),
            Some((FieldType::Int16, false))
        );
        assert_eq!(
            contract.enumerations["gtfs_pickup_dropoff"],
            ["0", "1", "2", "3"]
        );
        assert_eq!(contract.namespaces["cis:line_trip"].entity_kind, "trip");
    }

    #[test]
    fn every_key_names_declared_fields() {
        let contract = contract();
        for relation in &contract.relations {
            for key in &relation.primary_key {
                assert!(relation.field(key).is_some(), "{}.{key}", relation.name);
            }
            for fk in &relation.foreign_keys {
                let target = contract.relation(&fk.relation).expect("target relation");
                assert_eq!(fk.fields.len(), fk.target_fields.len());
                for (field, target_field) in fk.fields.iter().zip(&fk.target_fields) {
                    let source = relation.field(field).map(|f| f.ty);
                    assert_eq!(source, target.field(target_field).map(|f| f.ty), "{field}");
                }
            }
        }
    }

    #[test]
    fn versions_are_major_dot_minor() {
        assert_eq!(parse_version("5.2"), Some((5, 2)));
        assert_eq!(parse_version("5"), None);
        assert_eq!(parse_version("5.2.1"), None);
        assert_eq!(parse_version("+5.2"), None);
    }
}
