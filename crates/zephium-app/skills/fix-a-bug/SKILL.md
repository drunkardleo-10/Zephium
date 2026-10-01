---
name: fix-a-bug
description: Fix a bug or a failing test in the person's repository, from an issue, an error or a description: find the cause, change the code, show the tests passing.
tools: [start_part, create, finish]
---
# Fix a bug

## Parts
- **GitHub** — computer part (gh when installed; otherwise a browser part on github.com): read the issue with its comments, the linked pull requests and the failing output they quote.
- **Code** — computer part in the granted repository folder: find the code the issue points to (search_files, read_file), reproduce with the narrowest test command, make the smallest change that fixes the cause (edit_file; the person approves it), run the tests again and report the output.
- A failing test or a pasted error needs no GitHub part: the Code part runs the failing test first and reads the failure before it reads any code.
- Start GitHub first when the issue is the only lead; start both together when the request already names the file or the error.
- A folder the request names is asked about before you start. Without a readable repository folder, the Code part finishes with need allow_folder and the reply says in one sentence that the repository is needed.

## Result
- One **diff** per changed file: path, language, a one-line summary of the change, and its hunks.
- A **code** object only for a passage that explains the cause better than the diff does.
- A **draft** of the pull request or issue comment (destination github): what was wrong, what changed, how it was tested. Posting it goes through the app's Confirm.
- The **reply**: headline naming the cause in plain words, text with the fix and the test result (figures: tests passed, failed).
- Never claim tests pass without their output.
