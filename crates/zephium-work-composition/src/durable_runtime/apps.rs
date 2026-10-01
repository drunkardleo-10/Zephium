//! How a person works in their daily apps: from list views and the app's own
//! views, opening an item only when its row does not carry what the goal needs.

/// The page agent's note for the app a page task starts on, by host.
pub(super) fn note(host: &str) -> Option<&'static str> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let site = |known: &str| host == known || host.ends_with(&format!(".{known}"));
    Some(match host.as_str() {
        "app.slack.com" => SLACK,
        "mail.google.com" => GMAIL,
        "calendar.google.com" => CALENDAR,
        _ if site("linear.app") => LINEAR,
        _ if site("notion.so") || site("notion.com") => NOTION,
        "github.com" => GITHUB,
        _ => return None,
    })
}

const SLACK: &str = "\nSlack: work from its lists. The rail's Activity (mentions, threads and reactions) and DMs (each conversation with its latest message), and each channel by its sidebar row, open views whose rows carry sender, time and text; read those rows instead of opening each thread, and open a thread only when a needed reply is not in the list. Welcome and setup cards (add a handbook, invite teammates, connect apps) are not messages: never report them. Opening a view replaces the message pane: the fresh look shows it, so read the new pane rather than clicking again. Nothing unread is an answer: say so and read the latest messages shown. To find something older, use Slack's search box.";
const GMAIL: &str = "\nGmail: the inbox lists each conversation as a row with sender, subject, snippet and time; read the rows instead of opening each message, and open one only when its row lacks what the goal needs. To narrow the list, type a Gmail query such as is:unread newer_than:1d or is:starred into the search box and run it; the results list reads the same way.";
const CALENDAR: &str = "\nGoogle Calendar: the day view names each event's time and title on its own control; read the events from the grid, move between days with the view's previous, next or Today controls, and open an event only for details the grid lacks. The agenda (schedule) view lists several days as rows.";
const LINEAR: &str = "\nLinear: Inbox in the sidebar lists notifications about the person's issues, and My issues lists the issues assigned to them, each row with identifier, title, status and priority; read the rows and open an issue only when the goal needs its description or comments. Nothing in the Inbox is an answer: say so and read My issues.";
const NOTION: &str = "\nNotion: the sidebar and home list recent pages by title; open a page from them only when the goal needs its content.";
const GITHUB: &str = "\nGitHub: notifications, and the pull requests and issues assigned to the person (the Pull requests and Issues pages, Assigned tab), list rows with repository, title, state and time; read the rows and open one only when the goal needs its conversation.";

#[cfg(test)]
mod tests {
    #[test]
    fn daily_apps_read_from_their_lists_and_other_sites_have_no_note() {
        assert!(super::note("app.slack.com")
            .is_some_and(|note| note.contains("Activity") && note.contains("DMs")));
        assert!(super::note("mail.google.com").is_some_and(|note| note.contains("is:unread")));
        assert!(super::note("calendar.google.com").is_some());
        assert!(super::note("acme.linear.app").is_some() && super::note("www.notion.so").is_some());
        assert_eq!(super::note("www.google.com"), None);
        assert_eq!(super::note("gist.github.com"), None);
    }
}
