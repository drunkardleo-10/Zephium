---
name: review-a-change
description: Review a code change (a pull request link, a branch, or the uncommitted changes in a folder): find real problems, suggest exact fixes, and draft the review comment.
tools: [start_part, create, finish]
---
# Review a change

## Read
- A pull request link: one computer part, **Change**, with gh when installed (gh pr view with its description and comments, gh pr diff); otherwise a browser part on the pull request's Files view. When the repository is also a granted folder, the part reads the surrounding code there.
- A folder with no link: the Change part runs git status and git diff (and git diff against the main branch for a branch), then reads the changed files around each hunk.
- The part's brief: read the whole change, then the code each hunk touches (callers, tests, types), and return each problem with its path, line, severity (blocker, should fix, nit), what goes wrong and when, and the smallest fix. It runs the project's own tests or type check when the command is plain from the project's scripts, and reports the output. It never edits files or posts.

## Result
- One **list**, style requirements, titled with the verdict ("Two things before merging"): one item per problem, blockers first, title saying what breaks in a few words ("Token refresh races on two tabs"), detail with when it happens and the fix, priority high for blockers, from with the path and line as who, and the link to that line when the change is a pull request. At most twelve items; nits only when there are fewer than five other items. No item for style a formatter owns, and no praise items.
- A **diff** for each fix worth showing exactly, at most three, on the file it changes.
- A **draft** of the review comment (destination github, target_url the pull request): the verdict in one line, then the problems as a short list with file and line. Posting goes through the app's Confirm.
- The **reply**: headline with the verdict ("Ready after one fix"), text naming the one problem that matters most; figures for tests passed and failed only when the part ran them.
- When the change is sound, say so plainly: a list only when there is something to fix.
