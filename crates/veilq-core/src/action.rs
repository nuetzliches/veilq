//! What happens to a finding (ADR-0006). The default is `mask`: it needs no
//! key and gives nothing away.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    /// A plausible value of the same type and shape (M1).
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
    ) -> Result<String, Error> {
        match self {
            Self::Mask => Ok(format!("<{}>", entity.as_str())),
            Self::Pseudonymize => Ok(key.ok_or(Error::KeyMissing)?.pseudonym(entity, value)),
            // Fail closed: an action that is not built must not leave the
            // value where it was.
            Self::Fake => Err(Error::ActionNotBuilt(self.name())),
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
        assert_eq!(Action::default(), Action::Mask);
        assert_eq!(
            Action::Mask.replacement(&person, "Erika", None).unwrap(),
            "<PERSON>"
        );
        assert_eq!(
            Action::Remove.replacement(&person, "Erika", None).unwrap(),
            ""
        );
        assert!(
            Action::Pseudonymize
                .replacement(&person, "Erika", Some(&key))
                .unwrap()
                .starts_with("[PERSON:")
        );
        assert!(matches!(
            Action::Pseudonymize.replacement(&person, "Erika", None),
            Err(Error::KeyMissing)
        ));
        assert!(matches!(
            Action::Fake.replacement(&person, "Erika", None),
            Err(Error::ActionNotBuilt("fake"))
        ));
    }
}
