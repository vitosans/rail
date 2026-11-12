# Security and privacy

Where Rail keeps tokens and passwords, what it stores on your machine, when it loads remote content, and how the
daemon, the logs and AI agents are kept in bounds.

- [Tokens and passwords](#tokens-and-passwords)
- [What is stored locally](#what-is-stored-locally)
- [What Rail changes on the server](#what-rail-changes-on-the-server)
- [Remote content](#remote-content)
- [Mail and AI agents](#mail-and-ai-agents)
- [The daemon](#the-daemon)
- [Logs](#logs)

## Tokens and passwords

OAuth tokens and account passwords are never written to `config.toml`. They live in one of:

| Backend | Setting | Notes |
|---|---|---|
| The **OS keyring** | default; service `rail` | Secret Service on Linux, the Keychain on macOS. |
| A **private file** | `secrets = "file"` | `secrets.json` in the data directory, mode 0600, for machines without a Secret Service. |
| **1Password** | `secrets = "1password"` | Through the `op` CLI; see [Tokens in 1Password](configuration.md#tokens-in-1password). |

- Tokens, passwords, client secrets and authorization codes never appear in logs, error messages or terminal
  output.
- Passwords entered in the terminal or setup wizard are masked without local echo.
- Passwords are typed as `Secret<String>` and redacted at the type level (`Debug` output and logs display `<redacted>`).
- Files that hold mail or credentials are created owner-only (0600 files, 0700 directories) and
  written atomically.
- Stored credentials and tokens are removed when an account is removed (`rail account remove`).

## What is stored locally

### The store

For each account, `rail.db` holds:

- every label;
- the envelope (sender, recipients, subject, date, flags, labels, snippet) of every message inside
  `history_days` (the whole mailbox by default);
- the text of bodies for threads with mail newer than `body_cache_days`;
- attachment names, types and sizes. Never attachment contents, except those of a Gmail draft you
  open (below);
- drafts you write, with the paths of the files you attached. The files are read when the message
  is sent, so they must still be there then;
- for a snooze, when the thread comes back and the ids of the messages its archive took out of the
  inbox.

Other facts about what is kept:

- A draft saved to Gmail also lives in Gmail's Drafts, which sync brings in like other mail.
- HTML bodies are converted to text by Rail's own scanner without running scripts or loading
  anything remote, so syncing never fetches a tracking pixel.
- A body over 10 MiB is cut and says so.
- Mail that a search finds on its server outside the window is kept for 14 days after it was last
  searched for.
- The last time typed into the snooze chooser is kept in `snooze-last` in the state directory.

### Files outside the store

| File | Where | Removed |
|---|---|---|
| Attachments of an opened Gmail draft | Private files owned by the daemon: the `draft-files` folder of the data directory, 0600 files in 0700 folders (see [Writing mail](writing.md#files-of-an-imported-draft)) | When the draft is discarded; the daemon removes any no draft refers to when it starts. |
| A saved attachment (`s`, `S`, `w`) | A private (0600) file in `attachment_dir`, your download directory unless you set one | — |
| An opened attachment | A 0600 copy in a 0700 directory of its own under the cache directory (`~/.cache/rail/attachments/` on Linux) | A minute after you close the reader (so a viewer that starts slowly still finds it), or at once when you quit. Any copy older than 24 hours is swept when the daemon starts and before each open. |
| The editor's working file | The `compose` folder of the cache directory | When a save succeeds. One a crash left behind is removed when the mail screen next starts, once it is seven days old. |

A saved attachment never replaces a file that is there: a clash becomes `2-name.pdf`. A name
beginning with a dot or a dash loses it, so a save cannot hide a file in that directory or leave
one shaped like a command-line option.

On macOS, files Rail writes are quarantined like downloads.

All locations: [File locations](configuration.md#file-locations).

## What Rail changes on the server

Rail changes mail on the server only when you act on it (archive, trash, star, read, labels,
move), send, or save or discard a draft.

It never deletes mail permanently, except a Gmail draft you discard or send (a Microsoft 365 draft
you discard goes to Deleted Items).

The daemon is the only process that talks to Gmail, Microsoft 365, or IMAP/SMTP servers.

## Remote content

Remote content in mail is loaded only when you ask for it.

- `o` opens a message's HTML in your browser, after you confirm once per session.
- `g x` opens a link there, after you have seen its address.
- The reader draws a message's remote pictures only after `i`, for a sender you trusted with `I`,
  or with `remote_images = true`. The daemon fetches them as described in
  [Reading mail](reading.md#remote-pictures), and fetching one tells its sender you opened the
  mail.

## Mail and AI agents

Through MCP, mail content never reaches an agent today. Agents cannot supply passwords,
client secrets or configure IMAP/SMTP accounts; any credential pasted by an agent is dropped
immediately without being logged or echoed.

Text from outside Rail (preset files, provider and system error messages) comes back to the agent
marked as untrusted: quoted on one line, cut to 200 characters, and preceded by a notice that it
is data, not instructions.

The per-account [`ai_send`](configuration.md#ai_send-what-an-ai-agent-may-do-with-your-mail)
policy will govern sending once send tools exist. See [the honest limit](mcp.md#the-honest-limit)
for agents with shell access.

## The daemon

- It listens on a Unix socket in a directory only you can open (0700).
- The socket is 0600, and connections from other users are dropped.
- On screen, text Rail did not write (a provider's error, the system's) is shown on its own line,
  quoted, with control characters escaped.

## Logs

`rail.log` never holds tokens or mail at the default levels.

Rail keeps the HTTP and TLS libraries (`hyper`, `hyper_util`, `h2`, `reqwest`, `rustls`) at `warn`
whatever `RUST_LOG` says.

Setting a more specific target, such as `RUST_LOG=hyper::proto=debug` or
`RUST_LOG=rmcp::handler=warn`, overrides that and can write request lines, access tokens, mail or
tool arguments to `rail.log`. Treat the log as sensitive if you do.
