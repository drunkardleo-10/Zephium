---
name: explain-code
description: Explain how a piece of the person's code works (a file, a function, a flow in their repository, or code they pasted): what it does, how data moves through it, and why.
tools: [start_part, create, finish]
---
# Explain code

## Read
- Code pasted in the request is the subject: read it yourself, with no parts.
- A file, function or flow in a folder the request names: one computer part, **Code**, in that folder. Its brief: open the named file (search_files for a named symbol or error), follow the calls one step in and one step out (who calls it, what it calls), and return the path and exact line ranges that carry the idea, each with a one-line note. It reads only; it never edits or runs anything.
- A question about the whole folder ("what is this project") belongs to map-a-project.
- Explain from the code. Search only for how a library the code depends on behaves, when the code does not show it.

## Result
- A **diagram** of the flow when more than two pieces work together, read left to right in the order data moves: nodes named with the code's own names (functions, types, modules), kind service for code, store for databases and files, queue for channels, external for third-party APIs with their vendor host; edges labelled with what passes (Request, Rows, Token). At most twelve nodes.
- One **code** object per passage that carries the idea, at most three, in the order to read them: an exact excerpt of at most forty lines, titled with the file and the symbol ("session.rs · refresh"), with a plain note on each line that matters. Never an invented or tidied line.
- The **reply**: a headline that says what the code does in plain words ("Retries a payment until the bank confirms it"), text with how it does it and the one thing a reader would miss (a side effect, a race, a hidden cost), points for inputs, outputs and failure cases when they matter.
- No document narrating the code line by line, no sheet of terms.

## Follow-ups
- "Go deeper into X": the same Code part's reading of X, then revise the diagram in place and add its passage.
- "Why is it slow / wrong": that is fix-a-bug's job.
