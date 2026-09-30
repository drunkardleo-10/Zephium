//! The resource vocabulary shared by the compiler and persistent decoder.

use adblock::filters::network::NetworkFilterMask;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ResourceType {
    ChildDocument,
    Fetch,
    Font,
    Image,
    Media,
    Other,
    Ping,
    Script,
    StyleSheet,
    SvgDocument,
    TopDocument,
    Websocket,
}

impl ResourceType {
    // Canonical lexical order is part of artifact format 3. Derive the
    // decoder's cardinality bound from this same mapping, never a second
    // platform vocabulary that can drift independently.
    const MAPPING: [(NetworkFilterMask, Self); 12] = [
        (NetworkFilterMask::FROM_SUBDOCUMENT, Self::ChildDocument),
        (NetworkFilterMask::FROM_XMLHTTPREQUEST, Self::Fetch),
        (NetworkFilterMask::FROM_FONT, Self::Font),
        (NetworkFilterMask::FROM_IMAGE, Self::Image),
        (NetworkFilterMask::FROM_MEDIA, Self::Media),
        (NetworkFilterMask::FROM_OTHER, Self::Other),
        (NetworkFilterMask::FROM_PING, Self::Ping),
        (NetworkFilterMask::FROM_SCRIPT, Self::Script),
        (NetworkFilterMask::FROM_STYLESHEET, Self::StyleSheet),
        (NetworkFilterMask::FROM_OBJECT, Self::SvgDocument),
        (NetworkFilterMask::FROM_DOCUMENT, Self::TopDocument),
        (NetworkFilterMask::FROM_WEBSOCKET, Self::Websocket),
    ];

    pub(crate) const COUNT: usize = Self::MAPPING.len();

    pub(crate) fn for_mask(mask: NetworkFilterMask) -> Vec<Self> {
        Self::MAPPING
            .iter()
            .filter_map(|(flag, resource)| mask.contains(*flag).then_some(*resource))
            .collect()
    }
}
