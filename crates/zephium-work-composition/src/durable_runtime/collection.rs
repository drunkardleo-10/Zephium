use super::*;
use zephium_core::work::collection::{WorkBrowseCollection, WorkBrowseValue};

/// Host-selected record shape; contains no site code or new browser authority.
#[derive(Clone)]
pub struct WorkBrowseCollectionSchema {
    title: String,
    fields: Vec<SemanticExtractionFieldSchema>,
    max_items: usize,
    subject_url_field: Option<String>,
    subject_image_fields: Vec<String>,
}

impl TryFrom<&WorkBrowseCollection> for WorkBrowseCollectionSchema {
    type Error = WorkError;

    fn try_from(request: &WorkBrowseCollection) -> Result<Self, Self::Error> {
        request.validate()?;
        let mut fields = vec![
            SemanticExtractionFieldSchema::try_text("name".into(), true, 512)
                .map_err(|_| WorkError::Invalid)?,
        ];
        for column in &request.columns {
            let name = column.name.clone();
            let required = column.required;
            fields.push(
                match &column.value {
                    WorkBrowseValue::Text => {
                        SemanticExtractionFieldSchema::try_text(name, required, 1024)
                    }
                    WorkBrowseValue::Money { currencies } => {
                        SemanticExtractionFieldSchema::try_money(name, required, currencies.clone())
                    }
                    WorkBrowseValue::Url => {
                        SemanticExtractionFieldSchema::try_url(name, required, 2048)
                    }
                    WorkBrowseValue::ImageUrl => {
                        SemanticExtractionFieldSchema::try_image_url(name, required, 2048)
                    }
                }
                .map_err(|_| WorkError::Invalid)?,
            );
        }
        let mut schema = Self::try_new(
            request.title.clone(),
            fields,
            usize::from(request.max_items),
        )?;
        if let Some(column) = request
            .columns
            .iter()
            .find(|column| column.value == WorkBrowseValue::Url)
        {
            schema = schema.with_subject_url_field(&column.name)?;
        }
        for column in &request.columns {
            if column.value == WorkBrowseValue::ImageUrl {
                schema = schema.with_subject_image_field(&column.name)?;
            }
        }
        Ok(schema)
    }
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
            subject_image_fields: vec![],
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

    /// Selects an observed image field for Work's bounded image candidates.
    pub fn with_subject_image_field(mut self, name: &str) -> Result<Self, WorkError> {
        if self.subject_image_fields.len() >= 3
            || self.subject_image_fields.iter().any(|field| field == name)
            || !self.fields.iter().any(|field| {
                field.name() == name && field.kind() == SemanticExtractionValueKind::ImageUrl
            })
        {
            return Err(WorkError::Invalid);
        }
        self.subject_image_fields.push(name.to_owned());
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

    pub(super) fn append_browsing_fields(&self, objective: &mut String) -> Result<(), WorkError> {
        use std::fmt::Write as _;
        writeln!(
            objective,
            "\nCollection output_0: up to {} distinct records. Fields:",
            self.max_items
        )
        .map_err(|_| WorkError::Invalid)?;
        for field in &self.fields {
            let kind = match field.kind() {
                SemanticExtractionValueKind::Text => "text",
                SemanticExtractionValueKind::Url => "url",
                SemanticExtractionValueKind::ImageUrl => "image_url",
                SemanticExtractionValueKind::Money => "money",
                SemanticExtractionValueKind::Boolean => "boolean",
                SemanticExtractionValueKind::Unsigned => "unsigned",
                SemanticExtractionValueKind::TextList | SemanticExtractionValueKind::Rows => {
                    return Err(WorkError::Invalid)
                }
            };
            write!(
                objective,
                "{}: {kind} {}",
                field.name(),
                if field.required() {
                    "required"
                } else {
                    "optional"
                }
            )
            .map_err(|_| WorkError::Invalid)?;
            if let Some(currencies) = field.currencies() {
                write!(objective, " permitted_currencies={}", currencies.join(","))
                    .map_err(|_| WorkError::Invalid)?;
            }
            objective.push('\n');
        }
        Ok(())
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
                    Some(ArchivedValue::ImageUrl { value, sources })
                        if field.kind() == SemanticExtractionValueKind::ImageUrl =>
                    {
                        cell_evidence.extend(cite(sources)?);
                        if self
                            .subject_image_fields
                            .iter()
                            .any(|name| name == field.name())
                        {
                            let images = &mut subjects
                                .last_mut()
                                .ok_or(WorkError::Invalid)?
                                .image_candidates;
                            if !images.contains(value) {
                                images.push(value.clone());
                            }
                        }
                        WorkCellValue::Text {
                            text: value.clone(),
                        }
                    }
                    Some(ArchivedValue::Money {
                        amount,
                        currency,
                        sources,
                    }) if field.kind() == SemanticExtractionValueKind::Money
                        && field
                            .currencies()
                            .is_some_and(|currencies| currencies.contains(currency)) =>
                    {
                        cell_evidence.extend(cite(sources)?);
                        WorkCellValue::Money {
                            amount: amount.clone(),
                            currency: currency.clone(),
                            observed_at: None,
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
    fn work_collection_compiles_into_the_checked_native_record_shape() {
        let request: WorkBrowseCollection = serde_json::from_value(serde_json::json!({
            "title":"Products", "max_items":3, "columns":[
                {"name":"price","required":false,"value":{"kind":"money","permitted_currencies":["USD"]}},
                {"name":"product_url","required":true,"value":{"kind":"url"}},
                {"name":"image","required":false,"value":{"kind":"image_url"}}
            ]
        }))
        .unwrap();
        let schema = WorkBrowseCollectionSchema::try_from(&request).unwrap();
        assert_eq!(schema.fields[0].name(), "name");
        assert!(schema.fields[0].required());
        assert_eq!(schema.fields[1].currencies().unwrap(), ["USD"]);
        assert_eq!(schema.subject_url_field.as_deref(), Some("product_url"));
        assert_eq!(schema.subject_image_fields, ["image"]);
        let mut invalid = request.clone();
        invalid.columns[0].name = "name".into();
        assert!(WorkBrowseCollectionSchema::try_from(&invalid).is_err());
        invalid = request.clone();
        invalid.max_items = 33;
        assert!(WorkBrowseCollectionSchema::try_from(&invalid).is_err());
        invalid = request;
        invalid.columns[0].value = WorkBrowseValue::Money {
            currencies: vec!["USD".into(), "USD".into()],
        };
        assert!(WorkBrowseCollectionSchema::try_from(&invalid).is_err());
    }

    #[test]
    fn partial_product_record_preserves_anchored_link_and_unknown_fields() {
        let request = serde_json::from_value::<WorkBrowseCollection>(serde_json::json!({
            "title":"Product details", "max_items":1, "columns":[
                {"name":"price","required":false,"value":{"kind":"text"}},
                {"name":"pieces","required":false,"value":{"kind":"text"}},
                {"name":"dimensions","required":false,"value":{"kind":"text"}},
                {"name":"image","required":false,"value":{"kind":"image_url"}},
                {"name":"url","required":false,"value":{"kind":"url"}}
            ]
        }))
        .unwrap();
        let schema = WorkBrowseCollectionSchema::try_from(&request).unwrap();
        let mut rows: Vec<Vec<ArchivedField>> = serde_json::from_value(serde_json::json!([[
            {"name":"name","value":{"kind":"text","value":"Paris – City of Love","sources":[64]}},
            {"name":"pieces","value":{"kind":"text","value":"958","sources":[33]}},
            {"name":"image","value":{"kind":"image_url","value":"https://www.lego.com/product.png?width=800&height=800","sources":[26]}},
            {"name":"url","value":{"kind":"url","value":"https://www.lego.com/en-us/product/architecture-21064-21064#main-content","sources":[2]}}
        ]])).unwrap();
        let data = schema
            .comparison(&rows, &mut |ids| {
                Ok(ids
                    .iter()
                    .map(|id| match id {
                        64 => 0,
                        33 => 1,
                        26 => 2,
                        2 => 3,
                        _ => panic!(),
                    })
                    .collect())
            })
            .unwrap();
        data.validate(4).unwrap();
        let WorkArtifactDataV1::ComparisonMatrix {
            subjects, cells, ..
        } = data
        else {
            panic!()
        };
        assert_eq!(
            subjects[0].homepage.as_deref(),
            Some("https://www.lego.com/en-us/product/architecture-21064-21064#main-content")
        );
        assert!(matches!(cells[0][0].value, WorkCellValue::Unknown));
        assert!(matches!(cells[0][2].value, WorkCellValue::Unknown));
        assert_eq!(cells[0][1].evidence, [0, 1]);
        for invalid in [
            "javascript:alert(1)",
            "https://user:password@example.test/product#details",
            "http://example.test/product#details",
        ] {
            rows[0][3] = serde_json::from_value(serde_json::json!({"name":"url","value":{"kind":"url","value":invalid,"sources":[2]}})).unwrap();
            let data = schema.comparison(&rows, &mut |_| Ok(vec![0])).unwrap();
            assert!(data.validate(1).is_err());
        }
    }

    #[test]
    fn money_cells_keep_decimal_precision_currency_and_identity_citations() {
        let schema = WorkBrowseCollectionSchema::try_new(
            "Prices".into(),
            vec![
                SemanticExtractionFieldSchema::try_text("name".into(), true, 256).unwrap(),
                SemanticExtractionFieldSchema::try_money("price".into(), false, vec!["EUR".into()])
                    .unwrap(),
            ],
            3,
        )
        .unwrap();
        let rows: Vec<Vec<ArchivedField>> = serde_json::from_value(serde_json::json!([
            [{"name":"name","value":{"kind":"text","value":"Item A","sources":[1]}}, {"name":"price","value":{"kind":"money","amount":"1299.50","currency":"EUR","sources":[2]}}],
            [{"name":"name","value":{"kind":"text","value":"Item B","sources":[3]}}]
        ])).unwrap();
        let data = schema
            .comparison(&rows, &mut |ids| Ok(ids.iter().map(|id| id - 1).collect()))
            .unwrap();
        data.validate(3).unwrap();
        let WorkArtifactDataV1::ComparisonMatrix { cells, .. } = data else {
            panic!()
        };
        assert!(
            matches!(&cells[0][0].value, WorkCellValue::Money { amount, currency, observed_at: None } if amount == "1299.50" && currency == "EUR")
        );
        assert_eq!(cells[0][0].evidence, [0, 1]);
        assert!(matches!(cells[1][0].value, WorkCellValue::Unknown));
    }

    #[test]
    fn subject_url_is_host_selected_and_keeps_identity_and_destination_citations() {
        let schema = WorkBrowseCollectionSchema::try_new(
            "Items".into(),
            vec![
                SemanticExtractionFieldSchema::try_text("name".into(), true, 256).unwrap(),
                SemanticExtractionFieldSchema::try_url("product_url".into(), true, 512).unwrap(),
                SemanticExtractionFieldSchema::try_image_url("image".into(), true, 512).unwrap(),
            ],
            3,
        )
        .unwrap();
        assert!(schema.clone().with_subject_url_field("name").is_err());
        let schema = schema
            .with_subject_url_field("product_url")
            .unwrap()
            .with_subject_image_field("image")
            .unwrap();
        let rows: Vec<Vec<ArchivedField>> = serde_json::from_value(serde_json::json!([[
            {"name":"name","value":{"kind":"text","value":"Item A","sources":[1]}},
            {"name":"product_url","value":{"kind":"url","value":"https://shop.example.test/a","sources":[2]}},
            {"name":"image","value":{"kind":"image_url","value":"https://images.example.test/a.webp","sources":[3]}}
        ]])).unwrap();
        let data = schema
            .comparison(&rows, &mut |ids| Ok(ids.iter().map(|id| id - 1).collect()))
            .unwrap();
        data.validate(3).unwrap();
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
        assert_eq!(cells[0][1].evidence, [0, 2]);
        assert_eq!(
            subjects[0].image_candidates,
            ["https://images.example.test/a.webp"]
        );
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
