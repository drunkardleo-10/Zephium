use super::*;
use zephium_core::work::{execution_proposal::*, proposal::WorkPlanProposal};

const INSTRUCTIONS: &str = "Select capabilities for the exact current draft. Return every required node_N slot bound to its exact current draft key. Do not change objectives, dependencies or outputs. Select topology independent for independent ownership, or delegated with one explicit primary key for a coordinator and its direct children. A delegated primary MUST have synthesize capability; this explicitly requests the coordinator role, which synthesizes child findings. Other slots choose public_search, public_discovery or synthesize. Prefer public_search for finding current public facts and source citations: it runs one provider-native search using the explicitly reviewed OpenAI gpt-5.6-luna search model, without allocating a browser or disclosing dependency artifacts. Write a focused public search request naming the subject and the decision it must support; do not copy a long responsibility checklist or combine many alternative research topics into one query. Prefer primary sources and require source links. Use public_discovery only when inspecting or interacting with a web interface is necessary. Public search returns provider-attributed findings, not independently verified page evidence. Do not emit parent fields or a coordinate capability: the explicit topology defines those roles without contradictory declarations. For complex research with final synthesis, choose delegated and the final synthesis node as primary. A primary cannot be a dependency of any child, directly or indirectly. Data dependencies are separate from ownership: each node receives compact outputs only from its declared direct dependencies. Later research must use issue identifiers supplied by those handoffs; do not invent future discoveries now. Use public_discovery with a concise initial search_query for current public web research, only when every node output has source_mapped_needs_review review. Native research starts this exact approved public query in an isolated anonymous cookie store and follows observed public links; do not predeclare origins or paths. For dependent research, use a general initial query based on information already in the draft, without invented identifiers or template variables. Search text is public disclosure: omit private information, account data and secrets. Use synthesize for responsibilities completed from the objective and direct dependency artifacts. These capabilities cannot sign in, use private account data, write to services, or perform consequential effects. Capability selection and explicit topology are a proposal for human approval, never execution or completion.";

impl OpenAiWorkPlanner {
    pub(super) async fn execution_proposal(
        &self,
        input: WorkPlanningDisclosure,
    ) -> Result<WorkExecutionPlanningResult, WorkPlanningError> {
        let draft = input
            .context()
            .current_draft
            .as_ref()
            .ok_or(WorkPlanningError::Invalid)?;
        let schema = execution_schema(draft)?;
        let body = self.structured_request(
            serde_json::to_value(input.context()).map_err(|_| WorkPlanningError::Invalid)?,
            INSTRUCTIONS,
            "work_responsibilities",
            schema,
        )?;
        let (text, usage) = self.run_bounded(body, None, decode_response).await?;
        let value: Value =
            serde_json::from_str(&text).map_err(|_| WorkPlanningError::ProviderRefused(usage))?;
        if contains_secret(&value) {
            return Err(WorkPlanningError::ProviderRefused(usage));
        }
        let proposal = decode_execution_proposal(value, draft)
            .ok_or(WorkPlanningError::ProviderRefused(usage))?;
        Ok(WorkExecutionPlanningResult { proposal, usage })
    }
}

fn execution_schema(draft: &WorkPlanProposal) -> Result<Value, WorkPlanningError> {
    draft.validate().map_err(|_| WorkPlanningError::Invalid)?;
    let mut slots = serde_json::Map::new();
    for node in &draft.nodes {
        let capability = json!({"anyOf":[
            object(json!({"kind":{"const":"public_search","type":"string"},"query":{"type":"string","minLength":1,"maxLength":zephium_core::work::search::PUBLIC_SEARCH_MAX_QUERY_CHARS}})),
            object(json!({"kind":{"const":"public_discovery","type":"string"},"search_query":{"type":"string","minLength":1,"maxLength":zephium_core::work::runtime::WORK_PUBLIC_DISCOVERY_MAX_QUERY_CHARS}})),
            object(json!({"kind":{"const":"synthesize","type":"string"}}))
        ]});
        slots.insert(format!("node_{}", node.key), capability);
    }
    let keys: Vec<_> = draft.nodes.iter().map(|node| node.key).collect();
    let topology = json!({"anyOf":[
        object(json!({"kind":{"type":"string","const":"independent"}})),
        object(json!({"kind":{"type":"string","const":"delegated"},"primary":{"type":"integer","enum":keys}}))
    ]});
    Ok(object(
        json!({"topology":topology,"nodes":object(Value::Object(slots))}),
    ))
}

fn decode_execution_proposal(
    value: Value,
    draft: &WorkPlanProposal,
) -> Option<WorkExecutionProposal> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Response {
        topology: Topology,
        nodes: std::collections::BTreeMap<String, Capability>,
    }
    #[derive(serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    enum Topology {
        Independent {},
        Delegated { primary: u8 },
    }
    #[derive(serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
    enum Capability {
        PublicSearch { query: String },
        PublicDiscovery { search_query: String },
        Synthesize {},
    }
    let mut response: Response = serde_json::from_value(value).ok()?;
    if response.nodes.len() != draft.nodes.len() {
        return None;
    }
    let primary = match response.topology {
        Topology::Independent {} => None,
        Topology::Delegated { primary } => {
            if !draft.nodes.iter().any(|node| node.key == primary)
                || !matches!(
                    response.nodes.get(&format!("node_{primary}")),
                    Some(Capability::Synthesize {})
                )
            {
                return None;
            }
            Some(primary)
        }
    };
    let mut nodes = Vec::with_capacity(draft.nodes.len());
    for node in &draft.nodes {
        let slot = response.nodes.remove(&format!("node_{}", node.key))?;
        let capability = match slot {
            Capability::PublicSearch { query } => WorkCapabilityProposal::PublicSearch { query },
            Capability::PublicDiscovery { search_query } => {
                WorkCapabilityProposal::PublicDiscovery { search_query }
            }
            Capability::Synthesize {} if primary == Some(node.key) => {
                WorkCapabilityProposal::Coordinate
            }
            Capability::Synthesize {} => WorkCapabilityProposal::Synthesize,
        };
        nodes.push(WorkResponsibilityProposal {
            key: node.key,
            parent: primary.filter(|primary| *primary != node.key),
            capability,
        });
    }
    Some(WorkExecutionProposal { nodes })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn draft() -> WorkPlanProposal {
        WorkPlanProposal {
            nodes: (0..4)
                .map(|key| zephium_core::work::proposal::WorkNodeProposal {
                    key,
                    objective: format!("Research step {key}"),
                    dependencies: if key == 0 { vec![] } else { vec![key - 1] },
                    outputs: vec![zephium_core::work::WorkExpectedOutput {
                        name: "Findings".into(),
                        description: "Findings from source".into(),
                        review: zephium_core::work::WorkOutputReview::SourceMappedNeedsReview,
                    }],
                })
                .collect(),
        }
    }
    fn response() -> Value {
        json!({"topology":{"kind":"delegated","primary":3},"nodes":{
            "node_0":{"kind":"public_discovery","search_query":"public issue discovery"},
            "node_1":{"kind":"public_discovery","search_query":"public issue investigation"},
            "node_2":{"kind":"public_discovery","search_query":"public issue mitigation"},
            "node_3":{"kind":"synthesize"}
        }})
    }
    #[test]
    fn explicit_delegated_synthesis_primary_compiles_without_contradictory_parent_fields() {
        use zephium_core::work::{runtime::*, *};
        let draft = draft();
        let proposal = decode_execution_proposal(response(), &draft).unwrap();
        assert!(matches!(
            proposal.nodes[3].capability,
            WorkCapabilityProposal::Coordinate
        ));
        assert_eq!(proposal.nodes[3].parent, None);
        assert!(proposal.nodes[..3]
            .iter()
            .all(|node| node.parent == Some(3)));
        let draft = draft.mint().unwrap();
        let plan = WorkPlanRevision {
            context: None,
            author: WorkAuthor::PrimaryAgent,
            revision: WorkRevision::INITIAL,
            basis_revision: WorkRevision::INITIAL,
            draft,
        };
        assert!(proposal
            .compile(
                &plan,
                WorkExecutionLimits {
                    model_tokens: 256_000,
                    cost_micro_usd: 1_000_000,
                    operations: 256,
                    timeout_seconds: 900,
                    max_workers: 4
                }
            )
            .is_ok());
        let mut independent = response();
        independent["topology"] = json!({"kind":"independent"});
        let proposal = decode_execution_proposal(independent, &self::draft()).unwrap();
        assert!(proposal.nodes.iter().all(|node| node.parent.is_none()));
        assert!(matches!(
            proposal.nodes[3].capability,
            WorkCapabilityProposal::Synthesize
        ));
    }
    #[test]
    fn exact_slots_and_explicit_primary_refuse_missing_extra_and_conflicting_roles() {
        let draft = draft();
        let schema = execution_schema(&draft).unwrap();
        assert_eq!(
            schema["properties"]["nodes"]["required"],
            json!(["node_0", "node_1", "node_2", "node_3"])
        );
        for change in 0..6 {
            let mut value = response();
            match change {
                0 => {
                    value["nodes"].as_object_mut().unwrap().remove("node_0");
                }
                1 => value["nodes"]["node_4"] = json!({"kind":"synthesize"}),
                2 => value["topology"]["primary"] = json!(9),
                3 => value["topology"]["primary"] = json!(0),
                4 => value["nodes"]["node_3"]["parent"] = Value::Null,
                _ => value["nodes"]["node_3"]["kind"] = json!("coordinate"),
            }
            assert!(
                decode_execution_proposal(value, &draft).is_none(),
                "case {change}"
            );
        }
    }
}
