use super::{artifact::*, *};

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkBrowseCollection {
    pub title: String,
    pub columns: Vec<WorkBrowseColumn>,
    pub max_items: u8,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkBrowseColumn {
    pub name: String,
    pub value: WorkBrowseValue,
    pub required: bool,
    #[serde(default)]
    pub extraction: WorkBrowseExtraction,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkBrowseExtraction {
    Verbatim,
    #[default]
    Generate,
}

#[cfg_attr(feature = "ipc-types", derive(specta::Type))]
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkBrowseValue {
    Text,
    Money {
        #[serde(rename = "permitted_currencies")]
        currencies: Vec<String>,
    },
    Url,
    ImageUrl,
}

impl WorkBrowseCollection {
    pub fn validate(&self) -> Result<(), WorkError> {
        validate_text(&self.title, 512)?;
        if self.columns.is_empty()
            || self.columns.len() > MAX_ARTIFACT_CRITERIA
            || self.max_items == 0
            || usize::from(self.max_items) > MAX_ARTIFACT_SUBJECTS
            || (self.columns.len() + 1) * usize::from(self.max_items) > 256
        {
            return Err(WorkError::Invalid);
        }
        let mut names = BTreeSet::from(["name"]);
        let mut images = 0;
        for column in &self.columns {
            if column.name.is_empty()
                || column.name.len() > 64
                || !column.name.starts_with(|c: char| c.is_ascii_alphabetic())
                || !column
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_')
                || !names.insert(&column.name)
            {
                return Err(WorkError::Invalid);
            }
            match &column.value {
                WorkBrowseValue::Money { currencies } => {
                    if column.extraction == WorkBrowseExtraction::Verbatim {
                        return Err(WorkError::Invalid);
                    }
                    let mut unique = BTreeSet::new();
                    if currencies.is_empty()
                        || currencies.len() > 16
                        || currencies.iter().any(|code| {
                            code.len() != 3
                                || !code.bytes().all(|c| c.is_ascii_uppercase())
                                || !unique.insert(code)
                        })
                    {
                        return Err(WorkError::Invalid);
                    }
                }
                WorkBrowseValue::ImageUrl => images += 1,
                _ => {}
            }
        }
        if images > 3 {
            return Err(WorkError::Invalid);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_mode_is_additive_and_numeric_conversion_requires_generation() {
        let mut request: WorkBrowseCollection = serde_json::from_value(serde_json::json!({
            "title":"Observed product", "max_items":1,
            "columns":[{"name":"price","value":{"kind":"text"},"required":true}]
        }))
        .unwrap();
        assert_eq!(
            request.columns[0].extraction,
            WorkBrowseExtraction::Generate
        );
        request.columns[0].extraction = WorkBrowseExtraction::Verbatim;
        assert!(request.validate().is_ok());
        request.columns[0].value = WorkBrowseValue::Money {
            currencies: vec!["USD".into()],
        };
        assert_eq!(request.validate(), Err(WorkError::Invalid));
        request.columns[0].extraction = WorkBrowseExtraction::Generate;
        assert!(request.validate().is_ok());
    }
}
