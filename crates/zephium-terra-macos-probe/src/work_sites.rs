//! Closed public-only site inventory through the existing application runner.
//!
//! Inventory checks and fixed two/three-document reads, not commerce transactions,
//! authentication, arbitrary SPA navigation, or the six-site release matrix. No URL supplied
//! by the caller/page/model can expand this public retained-data allowlist.

use std::io::Write as _;
use zephium_agent_controller::*;
use zephium_agentic::*;

#[derive(Clone, Copy, Debug)]
pub(super) enum Site {
    React,
    ReactNavigation,
    ReactRoute,
    Commerce,
}

impl Site {
    pub(super) fn parse(value: &str) -> Result<Self, super::ProbeFailure> {
        match value {
            "react" => Ok(Self::React),
            "react-navigation" => Ok(Self::ReactNavigation),
            "react-route" => Ok(Self::ReactRoute),
            "commerce" => Ok(Self::Commerce),
            _ => Err(super::ProbeFailure::Authority),
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::React => "react_quick_start",
            Self::ReactNavigation => "react_exact_component_guide",
            Self::ReactRoute => "react_exact_three_document_route",
            Self::Commerce => "vercel_commerce_catalog",
        }
    }

    pub(super) const fn url(self) -> &'static str {
        match self {
            Self::React | Self::ReactNavigation | Self::ReactRoute => "https://react.dev/learn",
            Self::Commerce => "https://demo.vercel.store/",
        }
    }

    pub(super) const fn objective(self) -> &'static str {
        match self {
            Self::React => "Read the React Quick Start page. The initial viewport omits later headings, so call extract with trusted schema 1 and subtree scope targeting the current main content landmark's opaque ref. Locate that landmark if necessary. Return inventory as three complete exact heading names from the freshly delivered subtree evidence: Quick Start, Creating and nesting components, and Writing markup with JSX. Cite exactly one heading text source for each. Do not use initial extraction scope, navigation-link citations, paraphrases, navigation, or page modifications.",
            Self::ReactNavigation => super::work_navigation::OBJECTIVE,
            Self::ReactRoute => super::work_route::OBJECTIVE,
            Self::Commerce => "Read the public Vercel demo commerce home catalog. Call extract with trusted schema 1 and initial scope. Return inventory as the three complete exact product-link names for Acme Circles T-Shirt, Acme Drawstring Bag, and Acme Cup. Include all price and currency text when it is part of the accessible link name, copied exactly from delivered evidence. Cite exactly one link text source per item. Do not paraphrase, navigate, search, change a cart, or modify anything.",
        }
    }

    fn slot(self, role: SemanticRole, text: &str) -> Option<usize> {
        let (role_expected, names) = match self {
            Self::ReactNavigation | Self::ReactRoute => return None,
            Self::React => (
                SemanticRole::Heading,
                [
                    "Quick Start",
                    "Creating and nesting components",
                    "Writing markup with JSX",
                ],
            ),
            Self::Commerce => (
                SemanticRole::Link,
                ["Acme Circles T-Shirt", "Acme Drawstring Bag", "Acme Cup"],
            ),
        };
        if role != role_expected {
            return None;
        }
        names.iter().position(|name| match self {
            Self::React | Self::ReactNavigation | Self::ReactRoute => text == *name,
            Self::Commerce => text.strip_prefix(name).is_some_and(|rest| {
                // Require the catalog price in the same source; a menu label
                // or model-invented product name cannot satisfy this task.
                rest.trim_start().starts_with('$') && rest.bytes().any(|byte| byte.is_ascii_digit())
            }),
        })
    }

    pub(super) fn task(
        self,
        context: ContextIdentity,
    ) -> Result<Box<dyn AgentWorkTask>, AgentWorkFailure> {
        if matches!(self, Self::ReactNavigation) {
            return super::work_navigation::task(context);
        }
        if matches!(self, Self::ReactRoute) {
            return super::work_route::task(context);
        }
        Ok(Box::new(SiteTask {
            site: self,
            baseline: None,
            extraction: AgentWorkExtractionTask::try_new(
                vec![SemanticExtractionFieldSchema::try_text_list(
                    "inventory".into(),
                    true,
                    3,
                    256,
                )
                .map_err(|_| AgentWorkFailure::Contract)?],
                AgentAccountScope::Anonymous,
            )?
            .with_subtree_extraction()
            .with_source_roles(match self {
                Self::React | Self::ReactNavigation | Self::ReactRoute => {
                    SemanticReadRoleSelection::try_new(&[SemanticRole::Heading])
                        .map_err(|_| AgentWorkFailure::Contract)?
                }
                Self::Commerce => SemanticReadRoleSelection::ALL,
            }),
        }))
    }

    pub(super) fn verify_owned(self, result: &SemanticOwnedExtractionResult) -> bool {
        if matches!(self, Self::ReactNavigation) {
            return super::work_navigation::verify_owned(result);
        }
        if matches!(self, Self::ReactRoute) {
            return super::work_route::verify_owned(result);
        }
        if result.trust() != SemanticExtractionTrust::ModelMapped || result.fields().len() != 1 {
            return false;
        }
        let SemanticExtractedValue::TextList(list) = result.fields()[0].value() else {
            return false;
        };
        if list.items().len() != 3 {
            return false;
        }
        let mut slots = [false; 3];
        for item in list.items() {
            let Some(mut sources) = result.sources(item.source_span()) else {
                return false;
            };
            let Some(source) = sources.next() else {
                return false;
            };
            if sources.next().is_some() {
                return false;
            }
            let SemanticOwnedReadContent::Text(text) = &source.content else {
                return false;
            };
            if text != item.as_str() {
                return false;
            }
            let Some(slot) = self.slot(source.role, text) else {
                return false;
            };
            if std::mem::replace(&mut slots[slot], true) {
                return false;
            }
        }
        slots.into_iter().all(|seen| seen)
    }
}

impl Site {
    pub(super) const fn navigation_proposals(self) -> u32 {
        match self {
            Self::ReactNavigation => 1,
            Self::ReactRoute => 2,
            _ => 0,
        }
    }
}

struct SiteTask {
    site: Site,
    extraction: AgentWorkExtractionTask,
    baseline: Option<SemanticObservationId>,
}

impl AgentWorkTask for SiteTask {
    fn allows_subtree_extraction(&self) -> bool {
        matches!(self.site, Site::React)
    }
    fn extraction_schema(&self) -> Option<&SemanticExtractionSchema> {
        self.extraction.extraction_schema()
    }

    fn evaluate(
        &mut self,
        observation: &SemanticObservation,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        self.baseline = Some(observation.request().id());
        let mut slots = [0_u16; 3];
        let mut headings = 0_u16;
        let mut links = 0_u16;
        let mut nodes = 0_usize;
        for frame in observation.frames() {
            nodes += frame.nodes().len();
            for node in frame.nodes() {
                headings += u16::from(node.role() == SemanticRole::Heading);
                links += u16::from(node.role() == SemanticRole::Link);
                if let Some(slot) = node
                    .name()
                    .and_then(|text| self.site.slot(node.role(), text.as_str()))
                {
                    slots[slot] += 1;
                }
            }
        }
        writeln!(std::io::stdout().lock(), "work-site-observation: site={}; nodes={nodes}; headings={headings}; links={links}; task_matches={slots:?}; completeness={:?}; content=redacted", self.site.name(), observation.frames().first().map(SemanticSnapshot::completeness))
            .map_err(|_| AgentWorkFailure::Contract)?;
        self.extraction.evaluate(observation)
    }

    fn assess(
        &self,
        action: &SemanticPreparedAction,
    ) -> Result<AgentEffectAssessment, AgentWorkFailure> {
        self.extraction.assess(action)
    }

    fn attest_account(
        &self,
        context: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<AgentContextAccountBinding, AgentWorkFailure> {
        self.extraction.attest_account(context, now)
    }

    fn accept_extraction(
        &mut self,
        result: &SemanticExtractionResult<'_>,
    ) -> Result<AgentWorkTaskProgress, AgentWorkFailure> {
        let invalid = AgentWorkFailure::Contract;
        if self.baseline.is_none()
            || (matches!(self.site, Site::React) && Some(result.observation()) == self.baseline)
        {
            return Err(invalid);
        }
        let [field] = result.fields() else {
            return Err(invalid);
        };
        let SemanticExtractedValue::TextList(list) = field.value() else {
            return Err(invalid);
        };
        for (index, item) in list.items().iter().enumerate() {
            let sources = result.sources(item.source_span()).unwrap_or_default();
            for source in sources {
                let fragment = source.fragment();
                let (text_bytes, exact, slot) = match fragment.content() {
                    SemanticReadContent::Text(text) => (
                        text.len(),
                        text.as_str() == item.as_str(),
                        self.site.slot(fragment.role(), text.as_str()),
                    ),
                    _ => (0, false, None),
                };
                writeln!(std::io::stdout().lock(), "work-site-mapping: item={index}; source_count={}; role={:?}; source_bytes={text_bytes}; exact={exact}; slot={slot:?}; content=redacted", sources.len(), fragment.role())
                    .map_err(|_| invalid)?;
            }
        }
        if list.items().len() != 3 {
            return Err(invalid);
        }
        let mut slots = [false; 3];
        for item in list.items() {
            let Some([source]) = result.sources(item.source_span()) else {
                return Err(invalid);
            };
            let fragment = source.fragment();
            let SemanticReadContent::Text(text) = fragment.content() else {
                return Err(invalid);
            };
            if text.as_str() != item.as_str() {
                return Err(invalid);
            }
            let slot = self
                .site
                .slot(fragment.role(), text.as_str())
                .ok_or(invalid)?;
            if std::mem::replace(&mut slots[slot], true) {
                return Err(invalid);
            }
        }
        if !slots.into_iter().all(|seen| seen) {
            return Err(invalid);
        }
        self.extraction.accept_extraction(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_selection_is_fixed_task_authority_not_a_site_or_model_parameter() {
        let context = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            1_u128.into(),
            ContextKind::Owned,
        );
        let react = Site::React.task(context).unwrap();
        let commerce = Site::Commerce.task(context).unwrap();
        assert_eq!(
            react.extraction_schema().unwrap().source_roles(),
            SemanticReadRoleSelection::try_new(&[SemanticRole::Heading]).unwrap()
        );
        assert_eq!(
            commerce.extraction_schema().unwrap().source_roles(),
            SemanticReadRoleSelection::ALL
        );
        assert!(react.allows_subtree_extraction());
        assert!(!commerce.allows_subtree_extraction());
        assert!(!react.allows_actions_before_extraction());
        assert!(!react.allows_baseline_read());
    }

    #[test]
    fn public_retention_has_no_arbitrary_target_or_silent_fallback() {
        for value in [
            "https://react.dev/learn",
            "React",
            "commerce?user=private",
            "",
            "wikipedia",
        ] {
            assert!(Site::parse(value).is_err());
        }
        assert!(matches!(Site::parse("react"), Ok(Site::React)));
        assert!(matches!(Site::parse("commerce"), Ok(Site::Commerce)));
        assert!(matches!(
            Site::parse("react-navigation"),
            Ok(Site::ReactNavigation)
        ));
    }

    #[test]
    fn result_slots_require_task_semantics_and_source_roles() {
        assert_eq!(
            Site::React.slot(SemanticRole::Heading, "Quick Start"),
            Some(0)
        );
        assert_eq!(Site::React.slot(SemanticRole::Link, "Quick Start"), None);
        assert_eq!(
            Site::React.slot(SemanticRole::Heading, "Quick Start extra"),
            None
        );
        assert_eq!(
            Site::Commerce.slot(SemanticRole::Link, "Acme Cup$15.00 USD"),
            Some(2)
        );
        assert_eq!(Site::Commerce.slot(SemanticRole::Link, "Acme Cup"), None);
        assert_eq!(
            Site::Commerce.slot(SemanticRole::Link, "Acme Cupboard$15.00"),
            None
        );
        assert_eq!(
            Site::Commerce.slot(SemanticRole::Paragraph, "Acme Cup$15.00"),
            None
        );
    }
}
