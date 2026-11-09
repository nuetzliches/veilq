//! What happens to a finding (ADR-0006). The default is `mask`: it needs no
//! key and gives nothing away.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fake::Faker;
use crate::finding::EntityType;
use crate::{Error, PseudonymKey};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// `<PERSON>`.
    #[default]
    Mask,
    /// `[PERSON:ab12cd34]` (ADR-0001); needs a key.
    Pseudonymize,
    /// A plausible value of the same type and shape.
    Fake,
    /// Nothing: the span is dropped.
    Remove,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mask => "mask",
            Self::Pseudonymize => "pseudonymize",
            Self::Fake => "fake",
            Self::Remove => "remove",
        }
    }

    /// The text that replaces `value`, found as `entity`.
    pub fn replacement(
        self,
        entity: &EntityType,
        value: &str,
        key: Option<&PseudonymKey>,
        faker: &Faker,
    ) -> Result<String, Error> {
        match self {
            Self::Mask => Ok(format!("<{}>", entity.as_str())),
            Self::Pseudonymize => Ok(key.ok_or(Error::KeyMissing)?.pseudonym(entity, value)),
            Self::Fake => Ok(faker.fake(entity, value)),
            Self::Remove => Ok(String::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacements() {
        let person = EntityType::new("PERSON").unwrap();
        let key = PseudonymKey::new(b"0123456789abcdef".to_vec()).unwrap();
        let faker = Faker::new(*b"seed");
        assert_eq!(Action::default(), Action::Mask);
        assert_eq!(
            Action::Mask
                .replacement(&person, "Erika", None, &faker)
                .unwrap(),
            "<PERSON>"
        );
        assert_eq!(
            Action::Remove
                .replacement(&person, "Erika", None, &faker)
                .unwrap(),
            ""
        );
        assert!(
            Action::Pseudonymize
                .replacement(&person, "Erika", Some(&key), &faker)
                .unwrap()
                .starts_with("[PERSON:")
        );
        assert!(matches!(
            Action::Pseudonymize.replacement(&person, "Erika", None, &faker),
            Err(Error::KeyMissing)
        ));
        let fake = Action::Fake
            .replacement(&person, "Erika", None, &faker)
            .unwrap();
        assert!(!fake.is_empty() && fake != "Erika");
    }
}
