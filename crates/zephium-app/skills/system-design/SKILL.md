---
name: system-design
description: Design or explain a software architecture or stack, and compare providers or technologies for it.
tools: [create, revise, web_search, finish]
---
# System design

## Make it from what you know
- An architecture, a stack or a data flow is a making request: answer from your own knowledge without parts or searches. Search only for facts the design depends on that change (a current price, a limit, whether a service still exists), a few focused searches at most.

## Result
- One **diagram** of the system: every node a real component, named in at most 28 characters, with its kind; a vendor host (postgresql.org, vercel.com, cloudflare.com) whenever the node is a real product; layers for tiers (Client, Edge, App, Data, AI) when the design has them; labelled edges only for the main flow (Request, Embeds, Streams). At most 24 nodes; group small things.
- A **sheet** for the stack when it helps choose: columns Layer (text), Choice (entity, with the product's logo host on the row), Why (text, a phrase). One row per layer.
- A **plot** only for numbers worth comparing (monthly cost by stage), with a basis that says they are typical published figures when they are.
- The **reply** on top: headline naming the shape ("Edge API, queue workers, Postgres with pgvector"), text with the two or three decisions that matter, points for cautions.
- No document restating the diagram. Never sentences in sheet cells.

## Comparing providers for an existing design
- Read the diagram first (read_canvas with its id). Revise it: mark the recommended provider on each node it changes (vendor and a note such as "Cloudflare Workers"). Keep node ids so the person sees what changed.
- Add one **sheet** comparing the providers: first column entity with each provider's logo host; yes_no columns for capabilities the design needs (checks, not sentences); money columns with currency for comparable prices; best set on the columns where lower or higher wins.
- Reply with the recommendation and the reason in one sentence each.
