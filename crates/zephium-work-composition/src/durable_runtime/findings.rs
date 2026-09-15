use super::*;

pub(super) fn field_schema() -> Result<SemanticExtractionFieldSchema, WorkError> {
    SemanticExtractionFieldSchema::try_text_list("output_0".into(), true, 16, 1024)
        .map_err(|_| WorkError::Invalid)
}

pub(super) fn map_items(
    items: &[ArchivedText],
    cite: &mut impl FnMut(&[u16]) -> Result<Vec<u16>, WorkError>,
) -> Result<WorkArtifactDataV1, WorkError> {
    if items.is_empty() {
        return Err(WorkError::Unavailable);
    }
    let items = items
        .iter()
        .map(|item| {
            Ok(WorkFinding {
                claim: item.value().to_owned(),
                subject: None,
                evidence: cite(item.source_ids())?,
                confidence: WorkConfidence::Supported,
                detail: None,
                general_knowledge: false,
            })
        })
        .collect::<Result<Vec<_>, WorkError>>()?;
    Ok(WorkArtifactDataV1::Findings {
        subjects: vec![],
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_keep_each_items_sources_without_borrowing_collection_citations() {
        let value: ArchivedValue = serde_json::from_value(serde_json::json!({
            "kind": "text_list", "sources": [1], "items": [
                { "value": "The first condition applies.", "sources": [2, 3] },
                { "value": "A later version removes the second limitation.", "sources": [4] }
            ]
        }))
        .unwrap();
        let ArchivedValue::TextList { items, .. } = value else {
            panic!()
        };
        let data = map_items(&items, &mut |ids| Ok(ids.iter().map(|id| id - 2).collect())).unwrap();
        data.validate(3).unwrap();
        let WorkArtifactDataV1::Findings { items, .. } = data else {
            panic!()
        };
        assert_eq!(items[0].evidence, [0, 1]);
        assert_eq!(items[1].evidence, [2]);
        assert!(items[1].claim.starts_with("A later version"));
        assert!(matches!(
            map_items(&[], &mut |_| panic!()),
            Err(WorkError::Unavailable)
        ));
    }
}
