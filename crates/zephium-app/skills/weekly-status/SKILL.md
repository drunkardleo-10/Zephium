---
name: weekly-status
description: Write the person's or their team's weekly status from what really happened in Linear, GitHub and Slack this week: shipped, in progress, blocked, next.
tools: [start_part, create, finish]
---
# Weekly status

## Read (in one turn, in parallel)
- One part per place the work lives, named for the service with its site as service; the request's places, or Linear, GitHub and Slack when it names none. A connection the person has (gh, an MCP server) is offered by the app and used instead of the website.
  - **Linear** on linear.app: issues of the person's team completed in the last seven days, in progress, and blocked or overdue, each with its identifier, title, assignee, state and link.
  - **GitHub** on github.com (gh when installed): pull requests merged and still open in the last seven days for the person's repositories or team, with title, author, state, review state and link.
  - **Slack** on app.slack.com: in the team's channels and the person's threads, the last seven days' decisions, blockers and asks waiting on the team, each with channel, who, when, their words and the link.
- These parts read the person's own accounts; the app asks them first. Never put their text into a search.
- Each part's brief ends with: report in the digest and place nothing. You build every object from the digests, with no searches of your own after them.

## Result
- One **draft**, destination slack unless the person names email or another place: the status in four short sections, Shipped, In progress, Blocked, Next week, one line per item in plain words with its identifier or link, grouped by project when there are several. Only what the parts read; an empty section is left out. Posting goes through the app's Confirm.
- One **list**, style todo, titled "Needs attention", only when something is blocked or waiting on the person: each item verb-first ("Unblock the billing migration: Ana needs the schema"), with from (app, host, who, when, quote, link) and priority high when it blocks someone.
- The **reply**: a headline with the week's shape ("12 shipped, 2 blocked; billing slips a week"), figures for shipped, in progress and blocked.
- A place a part could not read shows its fix on its row, and the draft says nothing about it.
