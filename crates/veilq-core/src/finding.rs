//! Findings and reports: what CLI output and MCP tools return (ADR-0002).
//!
//! A finding names where personal data is and what kind, never what it is:
//! there is no field for the value, nor for an unkeyed hash of it (a hash of
//! a birth date or a postcode is reversed by trying all of them). The only
//! value-derived field is the keyed pseudonym, and only when the action is
//! `pseudonymize`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The report format's version. A change that breaks a reader is a new
/// version.
pub const REPORT_VERSION: u32 = 1;

/// An entity type: upper case ASCII letters, digits and `_`, starting with a
/// letter (`PERSON`, `IBAN_CODE`, a policy's own `FILE_NUMBER`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EntityType(String);

impl EntityType {
    pub fn new(name: &str) -> Result<Self, String> {
        let mut chars = name.chars();
        let valid = chars.next().is_some_and(|c| c.is_ascii_uppercase())
            && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        if valid {
            Ok(Self(name.to_owned()))
        } else {
            Err(format!(
                "`{name}` is not an entity type (upper case letters, digits and `_`, starting with a letter)"
            ))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl JsonSchema for EntityType {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "EntityType".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^[A-Z][A-Z0-9_]*$"
        })
    }
}

impl TryFrom<String> for EntityType {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        Self::new(&s)
    }
}

impl From<EntityType> for String {
    fn from(e: EntityType) -> String {
        e.0
    }
}

/// Where a finding is. Offsets are byte offsets into the segment's UTF-8
/// text; lines and columns count from 1, columns in characters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Location {
    /// A span of text: a plain file, a stream, a text node of a document.
    Span {
        /// The part of a container (`word/document.xml`, an attachment's
        /// name); absent for plain text.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Option<String>,
        start: u64,
        end: u64,
        line: u64,
        column: u64,
    },
    /// A cell of a table (CSV, a spreadsheet), with the span inside it.
    Cell {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sheet: Option<String>,
        /// Row from 1, the header row included.
        row: u64,
        /// Column from 1.
        column: u64,
        start: u64,
        end: u64,
    },
    /// A region of an image, a page or a frame, in pixels from the top left.
    Region {
        /// The part of a container the image is (`word/media/image1.png`,
        /// an attachment's name); absent for an image file.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Option<String>,
        /// Page or frame, from 1; absent for a single image.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame: Option<u64>,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}

/// One piece of personal data found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub entity: EntityType,
    pub location: Location,
    /// From 0 to 1.
    pub confidence: f32,
    /// The detector that found it: `pattern:<locale>.<rule>` for a built-in
    /// pattern, `policy:<name>` for one of the policy, later `ner`, `ocr`.
    pub detector: String,
    /// The keyed pseudonym (ADR-0001), only when the action is `pseudonymize`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pseudonym: Option<String>,
}

/// What a run found and how far its result can be trusted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub version: u32,
    pub findings: Vec<Finding>,
    /// True when the run had a known gap: a detector the policy names was
    /// off, or a format was handled with a loss (a PDF's text layer). A
    /// person should look at the output.
    pub review_required: bool,
    /// Why `review_required` is set, one sentence each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_reasons: Vec<String>,
}

impl Report {
    pub fn new(findings: Vec<Finding>) -> Self {
        Self {
            version: REPORT_VERSION,
            findings,
            review_required: false,
            review_reasons: Vec::new(),
        }
    }

    /// The JSON schema of the report, as `schema/report.schema.json` holds it.
    pub fn json_schema() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(Report)).expect("a schema serialises")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_type_names() {
        assert!(EntityType::new("PERSON").is_ok());
        assert!(EntityType::new("FILE_NUMBER_2").is_ok());
        for bad in ["", "person", "2FA", "FILE-NUMBER", "PERSÖN"] {
            assert!(EntityType::new(bad).is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<EntityType>("\"person\"").is_err());
    }

    #[test]
    fn a_finding_has_no_place_for_the_value() {
        let json = serde_json::json!({
            "entity": "PERSON",
            "location": {"kind": "span", "start": 0, "end": 16, "line": 1, "column": 1},
            "confidence": 0.9,
            "detector": "ner",
            "value": "Erika Mustermann"
        });
        assert!(serde_json::from_value::<Finding>(json).is_err());
    }

    #[test]
    fn report_round_trips() {
        let report = Report::new(vec![Finding {
            entity: EntityType::new("IBAN_CODE").unwrap(),
            location: Location::Cell {
                sheet: Some("Sheet1".into()),
                row: 2,
                column: 3,
                start: 0,
                end: 27,
            },
            confidence: 1.0,
            detector: "pattern".into(),
            pseudonym: None,
        }]);
        let text = serde_json::to_string(&report).unwrap();
        assert_eq!(serde_json::from_str::<Report>(&text).unwrap(), report);
    }
}
