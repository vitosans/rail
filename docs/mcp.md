# Using Rail from an AI agent (MCP)

How to connect an AI coding agent to Rail over MCP, what its tools do, and where their limits are.

- [Connecting an agent](#connecting-an-agent)
- [Tools](#tools)
- [What the tools cannot do](#what-the-tools-cannot-do)
- [Confirmation](#confirmation)
- [The honest limit](#the-honest-limit)

## Connecting an agent

Rail is an [MCP](https://modelcontextprotocol.io) server, so an AI coding agent (Claude Code,
Codex, Cursor and similar) can do what the wizard does, including creating the Google project or
the Microsoft app registration. Register it once:

```sh
claude mcp add rail -- rail mcp
```

Other clients: run `rail mcp` as a stdio server.

### The `setup` prompt

Then ask the agent to run Rail's `setup` prompt. It:

1. checks `rail_status`;
2. asks which providers you want;
3. walks through `rail_provider_guide` one step at a time: it runs the `gcloud` or `az` commands
   with your approval, and hands you the browser for the pages only you can complete (the Google
   consent screen, the client download, every login);
4. finishes with `rail_account_add` and `rail_wait`.

The steps are the same as in [Account setup](accounts.md).

## Tools

| Tool | What it does |
|---|---|
| `rail_status` | Everything `rail doctor` checks, plus the daemon's state. Read-only. |
| `rail_accounts_list` | Configured accounts, token presence, `ai_send` policy. Read-only. |
| `rail_presets_list` | Presets installed on this machine. Read-only. |
| `rail_preset_fetch` | A preset from an `https://` URL or an absolute file path. Read-only. |
| `rail_provider_guide` | The Google or Microsoft setup steps, as data. Read-only. |
| `rail_account_add` | Validates a new account, opens its login and returns the URL and an id. |
| `rail_account_login` | Logs in again to a configured account. |
| `rail_admin_consent` | Microsoft admin consent for one organisation. |
| `rail_wait` | Waits, at most five minutes per call, for a login or consent to finish. Giving up on a wait does not cancel the login. |
| `rail_cancel` | Cancels a login or consent that has not reached the save. |
| `rail_sync_status` | Each account's sync phase, counts, last success, last error and next attempt. No mail content. |
| `rail_sync_now` | Wakes one account's sync, or all, and returns at once. No mail content. |
| `rail_secrets_configure` | Moves tokens between the keyring, the file and 1Password. Needs your confirmation. |
| `rail_account_remove` | Removes an account's config entry and tokens; its mail stays. Needs your confirmation. |

## What the tools cannot do

- No tool returns mail content (subjects, senders, snippets or bodies), and none sends mail.
- No MCP tool archives, trashes, labels, snoozes or otherwise changes mail.
- No secret passes through the agent. `rail_account_add` takes the absolute path of the
  client-secret JSON you downloaded, an `op://` reference or a preset, never the secret itself,
  and a `client_secret` sent by an agent is refused.
- Tokens stay in the daemon.
- Tools refuse unknown arguments and relative file paths.

What an agent will be allowed to send is set per account by
[`ai_send`](configuration.md#ai_send-what-an-ai-agent-may-do-with-your-mail).

## Confirmation

`rail_account_remove` and `rail_secrets_configure` ask you to confirm in your agent client's own
dialog (MCP elicitation).

- The text is written by Rail, not the agent, with every name, address and vault quoted and shown
  in plain ASCII so look-alike characters cannot pass for others.
- It is bound to a digest of the exact plan: if anything changes before you answer, nothing runs
  and you are asked again.
- No tool input confirms on your behalf.
- Declining, dismissing or a client that cannot show the dialog changes nothing. The agent is told
  to ask you to run the equivalent terminal command (`rail account remove <name>`,
  `rail secrets set`) instead.

## The honest limit

This protects the MCP path only. An agent that can run shell commands runs them as you: it could
run `rail search`, read `rail.db`, drive `rail` in a pseudo-terminal or talk to the daemon's
socket, like any program you run. Rail cannot stop a program running as the same user.

There, the control is your agent client's approval of shell commands: keep it on, and read a
command that runs `rail` before you allow it.

See also [Mail and AI agents](security.md#mail-and-ai-agents).
