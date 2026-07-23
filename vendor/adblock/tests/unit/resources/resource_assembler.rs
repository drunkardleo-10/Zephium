#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn authored_redirect_mapping_is_parsed_and_assembled() {
        let mapping = concat!(
            "export default new Map([\n",
            "  [ 'noop.corpus.js', { alias: [ 'noop-alias.corpus.js', ",
            "'blank-alias.corpus.js' ] } ],\n",
            "  [ 'ignored.corpus.js', { params: [ 'one' ] } ],\n",
            "]);\n",
        );
        let properties = read_redirectable_resource_mapping(mapping);
        assert_eq!(properties.len(), 1);
        assert_eq!(properties[0].name, "noop.corpus.js");
        assert_eq!(
            properties[0].alias,
            ["noop-alias.corpus.js", "blank-alias.corpus.js"]
        );

        let resource =
            build_resource_from_file_contents(b"window.corpus = true;\r\n", &properties[0]);
        assert_eq!(resource.name, "noop.corpus.js");
        assert_eq!(
            resource.aliases,
            ["noop-alias.corpus.js", "blank-alias.corpus.js"]
        );
        assert_eq!(
            resource.kind,
            ResourceType::Mime(MimeType::ApplicationJavascript)
        );
        assert_eq!(
            BASE64_STANDARD.decode(resource.content).unwrap(),
            b"window.corpus = true;\n"
        );
    }

    #[test]
    fn authored_scriptlet_mapping_preserves_aliases_and_template_kind() {
        let scriptlets = concat!(
            "/// abort-corpus.js\n",
            "/// alias acorpus.js\n",
            "(function() { return '{{1}}'; })();\n",
            "\n",
            "/// plain-corpus.js\n",
            "window.corpus = true;\n",
            "\n",
        );
        let resources = read_template_resources(scriptlets);
        assert_eq!(resources.len(), 2);

        assert_eq!(resources[0].name, "abort-corpus.js");
        assert_eq!(resources[0].aliases, ["acorpus.js"]);
        assert_eq!(resources[0].kind, ResourceType::Template);
        assert_eq!(
            BASE64_STANDARD.decode(&resources[0].content).unwrap(),
            b"(function() { return '{{1}}'; })();\n"
        );

        assert_eq!(resources[1].name, "plain-corpus.js");
        assert!(resources[1].aliases.is_empty());
        assert_eq!(
            resources[1].kind,
            ResourceType::Mime(MimeType::ApplicationJavascript)
        );
        assert_eq!(
            BASE64_STANDARD.decode(&resources[1].content).unwrap(),
            b"window.corpus = true;\n"
        );
    }
}
