use super::*;

/// Host-selected record shape; contains no site code or new browser authority.
#[derive(Clone)]
pub struct WorkBrowseCollectionSchema {
    title: String,
    fields: Vec<SemanticExtractionFieldSchema>,
    max_items: usize,
    subject_url_field: Option<String>,
}

impl WorkBrowseCollectionSchema {
    /// The first field names each subject; remaining scalar fields become columns.
    pub fn try_new(
        title: String,
        fields: Vec<SemanticExtractionFieldSchema>,
        max_items: usize,
    ) -> Result<Self, WorkError> {
        if title.is_empty()
            || title.len() > 512
            || fields.len() < 2
            || fields.len() > MAX_ARTIFACT_CRITERIA + 1
            || max_items == 0
            || max_items > MAX_ARTIFACT_SUBJECTS
        {
            return Err(WorkError::Invalid);
        }
        SemanticActionText::try_new(title.clone()).map_err(|_| WorkError::Invalid)?;
        let first = &fields[0];
        if !first.required()
            || first.kind() != SemanticExtractionValueKind::Text
            || first.max_text_bytes().is_none_or(|bytes| bytes > 512)
        {
            return Err(WorkError::Invalid);
        }
        let schema = Self {
            title,
            fields,
            max_items,
            subject_url_field: None,
        };
        schema.extraction_field()?;
        Ok(schema)
    }

    /// Selects which source-backed URL identifies a subject in Work.
    pub fn with_subject_url_field(mut self, name: &str) -> Result<Self, WorkError> {
        if !self
            .fields
            .iter()
            .any(|field| field.name() == name && field.kind() == SemanticExtractionValueKind::Url)
        {
            return Err(WorkError::Invalid);
        }
        self.subject_url_field = Some(name.to_owned());
        Ok(self)
    }

    pub(super) fn extraction_field(&self) -> Result<SemanticExtractionFieldSchema, WorkError> {
        SemanticExtractionFieldSchema::try_rows(
            "output_0".into(),
            true,
            self.fields.clone(),
            self.max_items,
        )
        .map_err(|_| WorkError::Invalid)
    }

    pub(super) fn artifact(
        &self,
        profile: zephium_core::ids::ProfileId,
        output: &str,
        archive: &AgentWorkArchivedExtraction,
    ) -> Result<WorkArtifactDraft, WorkError> {
        if archive.descriptor().profile() != profile
            || archive.fields().len() != 1
            || archive.fields()[0].name() != "output_0"
        {
            return Err(WorkError::Invalid);
        }
        let ArchivedValue::Rows { items } = archive.fields()[0].value() else {
            return Err(WorkError::Invalid);
        };
        if items.is_empty() {
            return Err(WorkError::Unavailable);
        }
        if items.len() > self.max_items {
            return Err(WorkError::Capacity);
        }
        let extraction_id = zephium_core::work::WorkArtifactId::from(u128::from_be_bytes(
            archive.descriptor().id(),
        ));
        let mut evidence = Vec::<WorkEvidenceLink>::new();
        let mut cite = |sources: &[u16]| -> Result<Vec<u16>, WorkError> {
            sources
                .iter()
                .map(|source| {
                    if archive.source(*source).is_none() {
                        return Err(WorkError::Invalid);
                    }
                    let link = WorkEvidenceLink {
                        extraction_id,
                        source_id: *source,
                    };
                    let index = if let Some(index) = evidence.iter().position(|item| *item == link)
                    {
                        index
                    } else {
                        if evidence.len() >= MAX_ARTIFACT_EVIDENCE {
                            return Err(WorkError::Capacity);
                        }
                        evidence.push(link);
                        evidence.len() - 1
                    };
                    u16::try_from(index).map_err(|_| WorkError::Capacity)
                })
                .collect()
        };
        let data = self.comparison(items, &mut cite)?;
        data.validate(evidence.len())?;
        Ok(WorkArtifactDraft {
            output: output.to_owned(),
            title: self.title.clone(),
            data,
            evidence,
        })
    }

    fn comparison(
        &self,
        items: &[Vec<ArchivedField>],
        cite: &mut impl FnMut(&[u16]) -> Result<Vec<u16>, WorkError>,
    ) -> Result<WorkArtifactDataV1, WorkError> {
        let mut subjects = Vec::new();
        let mut cells = Vec::new();
        for row in items {
            if row
                .iter()
                .any(|cell| !self.fields.iter().any(|field| field.name() == cell.name()))
            {
                return Err(WorkError::Invalid);
            }
            let first = row.first().ok_or(WorkError::Invalid)?;
            if first.name() != self.fields[0].name() {
                return Err(WorkError::Invalid);
            }
            let ArchivedValue::Text {
                value: name,
                sources,
            } = first.value()
            else {
                return Err(WorkError::Invalid);
            };
            let name_evidence = cite(sources)?;
            subjects.push(WorkSubject {
                name: name.clone(),
                descriptor: None,
                homepage: None,
                image_candidates: vec![],
            });
            let mut row_cells = Vec::new();
            // Include identity evidence in each cell: it binds the value to its subject.
            for field in self.fields.iter().skip(1) {
                let mut cell_evidence = name_evidence.clone();
                let value = match row
                    .iter()
                    .find(|cell| cell.name() == field.name())
                    .map(ArchivedField::value)
                {
                    Some(ArchivedValue::Text { value, sources })
                        if field.kind() == SemanticExtractionValueKind::Text =>
                    {
                        cell_evidence.extend(cite(sources)?);
                        WorkCellValue::Text {
                            text: value.clone(),
                        }
                    }
                    Some(ArchivedValue::Url { value, sources })
                        if field.kind() == SemanticExtractionValueKind::Url =>
                    {
                        cell_evidence.extend(cite(sources)?);
                        if self.subject_url_field.as_deref() == Some(field.name()) {
                            subjects.last_mut().ok_or(WorkError::Invalid)?.homepage =
                                Some(value.clone());
                        }
                        WorkCellValue::Text {
                            text: value.clone(),
                        }
                    }
                    Some(ArchivedValue::Boolean { value, sources })
                        if field.kind() == SemanticExtractionValueKind::Boolean =>
                    {
                        cell_evidence.extend(cite(sources)?);
                        WorkCellValue::Presence { present: *value }
                    }
                    Some(ArchivedValue::Unsigned { value, sources })
                        if field.kind() == SemanticExtractionValueKind::Unsigned =>
                    {
                        cell_evidence.extend(cite(sources)?);
                        WorkCellValue::Text {
                            text: value.to_string(),
                        }
                    }
                    None if !field.required() => WorkCellValue::Unknown,
                    _ => return Err(WorkError::Invalid),
                };
                cell_evidence.sort_unstable();
                cell_evidence.dedup();
                row_cells.push(WorkCell {
                    value,
                    evidence: cell_evidence,
                    note: None,
                    general_knowledge: false,
                });
            }
            cells.push(row_cells);
        }
        let criteria = self
            .fields
            .iter()
            .skip(1)
            .map(|field| WorkCriterion {
                name: field.name().replace('_', " "),
                kind: if field.kind() == SemanticExtractionValueKind::Boolean {
                    WorkCriterionKind::Presence
                } else {
                    WorkCriterionKind::Text
                },
            })
            .collect();
        Ok(WorkArtifactDataV1::ComparisonMatrix {
            subjects,
            criteria,
            cells,
            notes: vec![
                "Observed records only; this collection does not establish complete site coverage."
                    .into(),
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_url_is_host_selected_and_keeps_identity_and_destination_citations() {
        let schema = WorkBrowseCollectionSchema::try_new(
            "Items".into(),
            vec![
                SemanticExtractionFieldSchema::try_text("name".into(), true, 256).unwrap(),
                SemanticExtractionFieldSchema::try_url("product_url".into(), true, 512).unwrap(),
            ],
            3,
        )
        .unwrap();
        assert!(schema.clone().with_subject_url_field("name").is_err());
        let schema = schema.with_subject_url_field("product_url").unwrap();
        let rows: Vec<Vec<ArchivedField>> = serde_json::from_value(serde_json::json!([[
            {"name":"name","value":{"kind":"text","value":"Item A","sources":[1]}},
            {"name":"product_url","value":{"kind":"url","value":"https://shop.example.test/a","sources":[2]}}
        ]])).unwrap();
        let data = schema
            .comparison(&rows, &mut |ids| Ok(ids.iter().map(|id| id - 1).collect()))
            .unwrap();
        data.validate(2).unwrap();
        let WorkArtifactDataV1::ComparisonMatrix {
            subjects, cells, ..
        } = data
        else {
            panic!()
        };
        assert_eq!(
            subjects[0].homepage.as_deref(),
            Some("https://shop.example.test/a")
        );
        assert_eq!(cells[0][0].evidence, [0, 1]);
    }

    #[test]
    fn collection_keeps_row_identity_citations_and_missing_values_distinct() {
        let schema = WorkBrowseCollectionSchema::try_new(
            "Observed items".into(),
            vec![
                SemanticExtractionFieldSchema::try_text("name".into(), true, 256).unwrap(),
                SemanticExtractionFieldSchema::try_text("displayed_price".into(), false, 128)
                    .unwrap(),
                SemanticExtractionFieldSchema::try_boolean("available".into(), false).unwrap(),
            ],
            3,
        )
        .unwrap();
        let rows: Vec<Vec<ArchivedField>> = serde_json::from_value(serde_json::json!([
            [
                {"name":"name","value":{"kind":"text","value":"Item A","sources":[1]}},
                {"name":"displayed_price","value":{"kind":"text","value":"£99.50","sources":[2]}},
                {"name":"available","value":{"kind":"boolean","value":false,"sources":[3]}}
            ],
            [{"name":"name","value":{"kind":"text","value":"Item B","sources":[4]}}]
        ]))
        .unwrap();
        let data = schema
            .comparison(&rows, &mut |ids| Ok(ids.iter().map(|id| id - 1).collect()))
            .unwrap();
        data.validate(4).unwrap();
        let WorkArtifactDataV1::ComparisonMatrix {
            subjects, cells, ..
        } = data
        else {
            panic!()
        };
        assert_eq!(subjects[0].name, "Item A");
        assert_eq!(subjects[1].name, "Item B");
        assert!(matches!(&cells[0][0].value, WorkCellValue::Text { text } if text == "£99.50"));
        assert_eq!(cells[0][0].evidence, [0, 1]);
        assert!(matches!(
            cells[0][1].value,
            WorkCellValue::Presence { present: false }
        ));
        assert_eq!(cells[0][1].evidence, [0, 2]);
        assert!(matches!(cells[1][0].value, WorkCellValue::Unknown));
        assert!(matches!(cells[1][1].value, WorkCellValue::Unknown));
        assert_eq!(cells[1][0].evidence, [3]);
        assert!(schema
            .comparison(&rows, &mut |_| Err(WorkError::Invalid))
            .is_err());
    }
}
