---
name: git-bug
description: Read, file, update and close this project's issues with git-bug. Use when asked about bugs, features, tasks or issues, or what to work on next.
---

# git-bug

Issues live in git-bug (v0.11.0), not in files. All commands below run from
the repo root.

## Approval

- **Creating an issue:** show Helly the title, body and labels and get a yes first.
- **Updating an issue:** comments, label changes and small title fixes are
  fine. If a change would significantly change what the issue means (its
  scope, its acceptance criteria, or what counts as done), get a yes first.
- **Closing an issue:** needs Helly's sign-off that the acceptance tests are
  complete. Don't close one just because the code landed.
- Filing an issue means deferring the work. Don't propose fixing it next.
- Never run `git bug push` or `git bug pull`. `origin` is on GitHub, and a push
  would publish the issues.
- Never run `git bug bug rm`.
- Don't use `git bug bug select`. It's hidden state that later commands
  pick up without saying so.

## Conventions

- Label every issue `bug` or `feature`. Add `low-priority` only when asked.
  `git bug label` lists the labels in use.
- Titles are conventional commits: `type(scope): description`, as in
  CLAUDE.md. `fix` for bugs, `feat` for features; `refactor`, `chore`,
  `ci` or `docs` when that's the work. Scope is the area (`contacts`,
  `activity`, `rx-log`); meshcore-rs work uses `meshcore-rs`. New tabs
  are `feat(tabs): add a … tab`.

## Reading

```sh
git bug bug status:open label:bug          # open bugs
git bug bug status:open label:feature      # open features
git bug bug status:open "hop count"        # full text search
git bug bug status:open -f json            # machine-readable output
git bug bug show <id>                      # details and comments
git bug bug comment <id>                   # comments only
```

IDs can be shortened to any unique prefix (`fc6c`).

## Writing

Always pass `--non-interactive` where it exists, or the command opens an
editor and hangs.

```sh
git bug bug new -t "fix(activity): title" -m "Body" --non-interactive
git bug bug label new <id> bug
git bug bug label rm <id> low-priority
git bug bug comment new <id> -m "Comment" --non-interactive
git bug bug title edit <id> -t "New title" --non-interactive
git bug bug status close <id>
git bug bug status open <id>
```

For multi-line bodies, pass a quoted multi-line string to `-m`. Don't use `-F`
with `bug new`: it takes the first line of the file as the title, overriding
`-t`, and drops that line from the body. `bug new` prints the new ID. Add the
label right after creating the issue, then check the result with
`git bug bug show <id>`.

## Gotchas

- An unknown subcommand prints the list help instead of an error. For
  example, `git bug bug comment add` lists bugs, because the subcommand is
  `new`. Read `git bug <cmd> --help` before using an unfamiliar subcommand.
- zsh doesn't word-split unquoted variables, so `for c in "comment new"; do git
  bug bug $c --help; done` passes a single argument and also prints the list
  help.
