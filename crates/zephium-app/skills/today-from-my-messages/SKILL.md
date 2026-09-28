---
name: today-from-my-messages
description: What the person needs to do today, from their own Slack, Gmail and other inboxes.
tools: [start_part, create, finish]
---
# Today from my messages

## Parts (in one turn)
- **Slack** — browser part on app.slack.com: unread direct messages, mentions and threads waiting on the person since yesterday; return each with channel or person, time, and their words.
- **Gmail** — browser part on mail.google.com: unread and starred mail in the primary inbox from the last two days that asks something of the person; skip newsletters and notifications.
- Add Calendar (calendar.google.com, today's events) or another inbox only when the person uses it or asks.
- These parts read the person's own accounts; the app asks them first. Never put their messages' text into a search.

## Result
- One **list**, style todo: one item per thing to do today, as a verb-first title ("Reply to Ana about the Q3 deck"), with from: the app (Slack, Gmail), its host, who, when, a short quote of their words and the link back; due when a time is stated; priority high for what blocks someone or is due today. Most urgent first. At most fifteen items; merge duplicates across apps.
- The **reply**: a headline with the count and the one thing to do first ("5 things today; Ana is waiting on the deck"), and no restatement of the list.
- Drafting a reply goes into a **draft** shaped like its destination only when the person asks; sending always goes through the app's Confirm.
