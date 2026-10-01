---
name: map-a-project
description: Map how a codebase fits together (its modules, how they connect, where a request enters, where to start reading) when asked for its map or architecture. What a folder holds is describe_project alone.
tools: [describe_project, start_part, create, finish]
---
# Map a project

## Read (in one turn)
- describe_project on the folder: its project object (stack, structure, scripts, git state) is the first half of the map.
- One computer part, **Architecture**, in the same folder. Its brief: find the entry points (main, the server's routes, the app's root component, the CLI's commands), the modules or packages and which of them import which, and the external services the code calls (databases, queues, third-party APIs, found in config and client setup). Return each module with its path and one line on its job, each dependency between modules, and each external service with its host. It reads only and stops after about twenty files.

## Result
- The **project** object from describe_project, as it is.
- One **diagram**, left to right from where a request enters to where data lands: layers for the tiers the code really has (Client, API, Domain, Data, Services), nodes for its modules named as the code names them, kind service for code, store and queue for storage, external for third-party APIs with their vendor host (stripe.com, postgresql.org) so their logos show. Edges for the main flow only, labelled with what passes. At most twenty nodes: group small modules.
- The **reply**: a headline naming the shape ("A SvelteKit app over a Rust API and Postgres"), text naming the two or three files to read first and why, points for what stands out (a missing test suite, a module everything depends on, a stale dependency) only when the reading showed it.
- No sheet restating the stack the project object already shows; no document.

## Follow-ups
- "Explain X" goes to explain-code; "fix", "failing tests" to fix-a-bug; "review my changes" to review-a-change.
