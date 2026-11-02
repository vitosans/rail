# Commands

Every `rail` command, its flags and its exit codes.

- [Overview](#overview)
- [`rail`](#rail)
- [Accounts](#accounts)
- [`rail secrets set`](#rail-secrets-set)
- [`rail sync`](#rail-sync)
- [`rail search`](#rail-search)
- [`rail doctor`](#rail-doctor)
- [The daemon](#the-daemon)
- [`rail mcp`](#rail-mcp)

## Overview

| Command | What it does |
|---|---|
| `rail` | Opens the mail screen, or the setup wizard when no account is configured. |
| `rail setup` | Opens the setup wizard to add an account. |
| `rail account add` | Adds an account, through the wizard or with flags. |
| `rail account login <account>` | Logs in again to an existing account. |
| `rail account list` | Lists accounts and whether their tokens are stored. |
| `rail account remove <account>` | Removes an account's config entry and tokens. |
| `rail secrets set` | Changes where tokens are kept. |
| `rail sync` | Wakes the sync and shows progress. |
| `rail search <query>` | Searches the store and each account's server. |
| `rail doctor` | Checks that everything is in order. |
| `rail daemon status`, `stop`, `run`, `install`, `uninstall` | Manage the background daemon. |
| `rail mcp` | Serves Rail's tools to an AI agent. |

## `rail`

Opens the mail screen when an account is configured, otherwise the setup wizard.

- Both need a terminal: in a pipe they print a hint and exit 2.
- A bad `config.toml` or `[keys]` entry is reported with its line before the terminal changes
  (exit 1).

See [Reading mail](reading.md) and [The setup wizard](accounts.md#the-setup-wizard).

## Accounts

| Command | What it does |
|---|---|
| `rail setup` | Opens the setup wizard to add an account. |
| `rail account add [gmail\|m365]` | With no flags, opens the wizard (with that provider chosen, if given). |
| `rail account add <gmail\|m365> --address <addr> --client-id <id> [--client-secret <secret>] [--tenant <tenant>] [--name <name>] [--color <colour>] [--store-secret-in-1password]` | Adds an account without the wizard, then opens the browser login. A partial set of flags exits 1 and names the missing one. |
| `rail account login <account>` | Logs in again to an existing account (name or address) and replaces its tokens; `config.toml` is untouched. |
| `rail account list` | Lists accounts and whether their tokens are stored, without reading the tokens. |
| `rail account remove <account>` | Removes the account's config entry and tokens; its mail stays in the store. Asks `Remove? [y/N]`; refuses (exit 1) when stdin or stdout is not a terminal. |

Where the `--client-id` and the rest come from: [Account setup](accounts.md).

## `rail secrets set`

```sh
rail secrets set <keyring|file|1password> [--vault <vault>] [--account <account>] [--no-migrate]
```

Changes where tokens are kept and moves them there.

- `--no-migrate` changes the setting only; each account then needs `rail account login`.
- Asks `Change? [y/N]`; interactive only.

See [Tokens](security.md#tokens) and [Tokens in 1Password](configuration.md#tokens-in-1password).

## `rail sync`

```sh
rail sync [<account>] [--no-wait]
```

Wakes the sync for one account or all and shows progress (counts only, never mail) until caught
up.

- Newest mail arrives first, so recent mail is searchable within seconds. A first sync of a large
  mailbox takes hours (about 250 messages a minute, under Gmail's quota). Set
  [`history_days`](configuration.md#sync-settings) to sync only recent mail.
- Ctrl-C stops watching, not the sync.
- If nothing changes for 10 minutes it stops watching (the sync goes on).
- If the daemon restarts, or restarts its sync, during the watch, it says which and waits for the
  new pass.
- `--no-wait` wakes it and exits 0.

| Exit code | Meaning |
|---|---|
| 0 | Caught up (or `--no-wait`). |
| 1 | A sync failed. |
| 2 | An account needs `rail account login`. |
| 130 | Ctrl-C. |

## `rail search`

```sh
rail search <query> [--account <account>] [--limit N]
```

Searches the local store and each account's mailbox on its server (Gmail's search, Microsoft
Graph's `$search`).

- One line per thread, newest first: date (UTC), account, sender, subject. Never a snippet or
  body.
- `--limit` is 1 to 100 (default 20).
- Results go to stdout, notes to stderr.
- Gmail's search syntax works for Gmail accounts. Microsoft 365 takes `from:`, `subject:`, `to:`,
  `body:` and plain words (see [Microsoft 365 mail](accounts.md#microsoft-365-mail)).
- When a server is busy or refuses the connection (`not searched on Gmail` or
  `not searched on Microsoft 365` says which), it is asked once more after a short wait. The local
  results still print.
- Exits 1 for an unknown account, or when nothing was found and a server search failed.

## `rail doctor`

A single health check. It reports on the config, the store, the token backend, each account's
tokens, `ai_send` and sync, the daemon and the login unit.

- It notes a `general.layout` this version treats as `auto`, and prints a `theme` line when
  `general.theme` is set.
- It exits non-zero for config, store, token-backend and token-read errors, and for an account
  that needs a login or whose sync failed for good.
- Warnings and the login-unit line do not change the status.

## The daemon

| Command | What it does |
|---|---|
| `rail daemon status` | Shows whether the daemon runs, with pid, version and build, load and socket. Exits 1 when none runs; never starts one. |
| `rail daemon stop` | Asks the daemon to exit once any save in flight is written; waits up to 35 seconds. |
| `rail daemon run` | Runs the daemon in the foreground (Ctrl-C or SIGTERM stops it cleanly). Rarely needed. |
| `rail daemon install` | Writes and enables a systemd user unit (Linux) or launchd agent (macOS) for the user running it that starts the daemon at login; stops a daemon already running first. If enabling fails it puts the previous state back and exits 1; if the daemon does not answer within 10 seconds it exits 1 with the unit's state (the unit stays installed). |
| `rail daemon uninstall` | Stops, disables and removes the login unit. |

How the daemon behaves: [The background daemon](configuration.md#the-background-daemon).

## `rail mcp`

Serves Rail's tools to an AI agent over MCP on stdin and stdout. See [MCP](mcp.md).
