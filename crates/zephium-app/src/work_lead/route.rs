//! Which way a part reaches a service the person uses: through their own
//! connection (an installed CLI or an MCP server they added), offered once
//! per work, or else through the website in their session. The run decides
//! this, not the model.
use zephium_core::work::parts::{WorkHelperV1, WorkPartServiceV1};
use zephium_ipc::work::WorkServerV1;

/// Services people reach both ways, by the name the lead may use for them.
const SERVICES: [(&str, &str, &str); 8] = [
    ("slack", "Slack", "app.slack.com"),
    ("github", "GitHub", "github.com"),
    ("gmail", "Gmail", "mail.google.com"),
    ("calendar", "Google Calendar", "calendar.google.com"),
    ("notion", "Notion", "www.notion.so"),
    ("linear", "Linear", "linear.app"),
    ("figma", "Figma", "www.figma.com"),
    ("discord", "Discord", "discord.com"),
];

/// A connection the person has for a service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Offer {
    /// What the part's service names, for the connection helper's tools.
    pub connection: String,
    /// "Use Slack (MCP)?"
    pub prompt: String,
    /// "Use Slack": the connection helper's own first-use answer.
    pub yes: String,
}

/// The service a part is about, in lower case: "slack" for app.slack.com,
/// a connection named Slack or a part titled Slack.
pub(crate) fn brand(service: Option<&WorkPartServiceV1>, title: &str) -> Option<String> {
    if let Some(host) = service.and_then(|s| s.host.as_deref()) {
        if host == "mail.google.com" {
            return Some("gmail".into());
        }
        if host == "calendar.google.com" {
            return Some("calendar".into());
        }
        let labels: Vec<&str> = host.split('.').collect();
        return labels
            .len()
            .checked_sub(2)
            .and_then(|at| labels.get(at))
            .map(|label| (*label).to_owned());
    }
    let named = service
        .and_then(|s| s.connection.as_deref())
        .unwrap_or(title)
        .to_lowercase();
    let word = named
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| !w.is_empty())?
        .to_owned();
    Some(if word == "gh" { "github".into() } else { word })
}

/// The website a service lives on, when it is a known one.
pub(crate) fn site(brand: &str) -> Option<&'static str> {
    SERVICES
        .iter()
        .find(|(key, ..)| *key == brand)
        .map(|(.., host)| *host)
}

/// The person's connection for the service: gh for GitHub when it is
/// installed and signed in, else an enabled MCP server named for it.
pub(crate) fn offer(brand: &str, gh_ready: bool, servers: &[WorkServerV1]) -> Option<Offer> {
    if brand == "github" && gh_ready {
        return Some(Offer {
            connection: "GitHub".into(),
            prompt: crate::work_connections::gh::ASK.into(),
            yes: crate::work_connections::gh::USE.into(),
        });
    }
    let server = servers.iter().find(|server| {
        let id = server.id.to_lowercase();
        let name = server.name.to_lowercase();
        server.enabled
            && !id.is_empty()
            && (id == brand
                || name
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| w == brand)
                || id.split('-').any(|w| w == brand))
    })?;
    Some(Offer {
        connection: server.name.clone(),
        prompt: format!(
            "Use {} (MCP)? Its tools read {} as you in this work.",
            server.name, server.name
        ),
        yes: format!("Use {}", server.name),
    })
}

/// Where a part goes once the person answered, or when there is nothing to
/// offer: a connection part with no connection works on the website.
pub(crate) fn settle(
    helper: WorkHelperV1,
    brand: &str,
    offer: Option<&Offer>,
    accepted: bool,
) -> Option<(WorkHelperV1, WorkPartServiceV1)> {
    match (helper, offer, accepted) {
        (WorkHelperV1::Browser | WorkHelperV1::Connection, Some(offer), true) => Some((
            WorkHelperV1::Connection,
            WorkPartServiceV1 {
                host: None,
                connection: Some(offer.connection.clone()),
            },
        )),
        (WorkHelperV1::Connection, _, _) => site(brand).map(|host| {
            (
                WorkHelperV1::Browser,
                WorkPartServiceV1 {
                    host: Some(host.into()),
                    connection: None,
                },
            )
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_ipc::work::WorkServerTransportV1;

    fn server(id: &str, name: &str, enabled: bool) -> WorkServerV1 {
        WorkServerV1 {
            id: id.into(),
            name: name.into(),
            transport: WorkServerTransportV1::Stdio {
                command: "/usr/local/bin/slack-mcp".into(),
                args: vec![],
                env: vec![],
            },
            enabled,
        }
    }
    fn host(host: &str) -> WorkPartServiceV1 {
        WorkPartServiceV1 {
            host: Some(host.into()),
            connection: None,
        }
    }

    #[test]
    fn a_service_is_named_by_its_site_its_connection_or_its_part() {
        assert_eq!(
            brand(Some(&host("app.slack.com")), "Today").as_deref(),
            Some("slack")
        );
        assert_eq!(
            brand(Some(&host("mail.google.com")), "Mail").as_deref(),
            Some("gmail")
        );
        let gh = WorkPartServiceV1 {
            host: None,
            connection: Some("gh".into()),
        };
        assert_eq!(brand(Some(&gh), "Code").as_deref(), Some("github"));
        assert_eq!(brand(None, "Linear issues").as_deref(), Some("linear"));
    }

    #[test]
    fn a_connection_is_offered_before_the_website_and_never_invented() {
        let servers = [
            server("slack", "Slack", true),
            server("notion", "Notion", false),
        ];
        let slack = offer("slack", false, &servers).unwrap();
        assert_eq!(
            slack.prompt,
            "Use Slack (MCP)? Its tools read Slack as you in this work."
        );
        assert_eq!(slack.yes, "Use Slack");
        assert_eq!(offer("notion", false, &servers), None);
        assert_eq!(offer("github", true, &servers).unwrap().yes, "Use GitHub");
        assert_eq!(offer("github", false, &servers), None);
        assert_eq!(offer("sla", false, &servers), None);

        let used = settle(WorkHelperV1::Browser, "slack", Some(&slack), true).unwrap();
        assert_eq!(used.0, WorkHelperV1::Connection);
        assert_eq!(used.1.connection.as_deref(), Some("Slack"));
        assert_eq!(
            settle(WorkHelperV1::Browser, "slack", Some(&slack), false),
            None
        );
        let website = settle(WorkHelperV1::Connection, "linear", None, false).unwrap();
        assert_eq!(website.0, WorkHelperV1::Browser);
        assert_eq!(website.1.host.as_deref(), Some("linear.app"));
    }
}
