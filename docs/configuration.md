# Configuration

`config.toml` and every setting in it, where Rail keeps its files, and how the background daemon
behaves.

- [The config file](#the-config-file)
- [`[general]`](#general)
- [`[keys]`](#keys)
- [`[[saved_searches]]`](#saved_searches)
- [`[[link_actions]]`](#link_actions)
- [`[[accounts]]`](#accounts)
- [Sync settings](#sync-settings)
- [`ai_send`: what an AI agent may do with your mail](#ai_send-what-an-ai-agent-may-do-with-your-mail)
- [Tokens in 1Password](#tokens-in-1password)
- [File locations](#file-locations)
- [The background daemon](#the-background-daemon)

## The config file

The wizard and `rail account add` write `config.toml` for you and keep any comments you add. A
minimal file:

```toml
[general]
secrets = "keyring"            # "keyring" (default), "file" or "1password"

[[accounts]]
name = "work"
kind = "gmail"                 # "gmail" or "m365"
address = "you@example.com"
client_id = "XXXX.apps.googleusercontent.com"
client_secret = "GOCSPX-..."   # Gmail only; may be an op:// reference
color = "blue"
```

- Unknown keys and wrong types fail to load, with the file's path and line number.
- Edits are picked up by the running daemon within seconds.
- An edit that leaves the file invalid is logged and the running syncs carry on unchanged.

Where the file lives: [File locations](#file-locations).

## `[general]`

| Key | Default | Meaning |
|---|---|---|
| `secrets` | `"keyring"` | Where tokens are kept: `"keyring"`, `"file"` or `"1password"`. Change it with `rail secrets set` to move existing tokens. |
| `poll_interval` | `"60s"` | How often each account is checked for new mail. At least `5s`. |
| `history_days` | `0` | How many days of mail the store keeps; `0` is the whole mailbox. |
| `body_cache_days` | `30` | Bodies of threads with mail this recent are fetched ahead of time. |
| `icons` | `"unicode"` | `"unicode"` or `"ascii"`. The mail screen's glyphs; `"ascii"` swaps them for `*`, `+`, `@`, `ok`, `~`, `\|`, `v`, `>` and `-` (the rules), for fonts without them. |
| `theme` | unset | A scheme name as `:theme` lists it (case does not matter; names may contain spaces). Checked when the mail screen starts; a misspelling names the nearest scheme. |
| `accent` | unset | The accent colour: a palette slot `0` to `15` of the scheme (a number, or a string such as `"5"`), a colour name such as `"blue"`, or `"#rrggbb"`. Without truecolor a `"#rrggbb"` accent is not applied. |
| `split_threshold` | `140` | At this many columns and wider the reader opens beside the list (in the mail screen). |
| `hints` | `true` | `false` turns the key hints off: the bottom row keeps only notices and each account's state, and the key boxes' rows go to the panes. `:hints` in the palette toggles it and saves it. |
| `clock` | `true` | `false` hides the time of day at the right edge of the header. `:clock` in the palette toggles it and saves it. |
| `time` | `"24h"` | `"24h"` writes times of day as `14:05`, `"12h"` as `2:05 pm` (the list, the header's `Synced` and its clock, the Drafts list, `:errors`, the reader's title block and cards (`04:32` today, `Thu 08:55` yesterday and this week), and the `Date:` header line `H` shows). `:time` in the palette toggles it and saves it. |
| `dim_read` | `"dot"` | Read threads: `"dot"` keeps the small muted dot, `"subject"` also dims the subject, `"all"` dims the sender, subject, count and time. `:dim-read` cycles the modes and saves the choice. |
| `keymap` | `"rail"` | The key preset every action starts from: `"rail"`, `"vim"`, `"mutt"`, `"emacs"`, `"gmail"` or `"outlook"` (see [Keys](keys.md#key-presets)). `[keys]` goes over it. `:keymap` switches, asks whether to keep the new keys, and saves them if you do. |
| `field_keys` | `"readline"` | How text fields edit: `"readline"` (bash's keys), `"vi"` (insert and normal mode) or `"basic"` (arrows, `Home`/`End`, `Backspace` and `Delete` only). The same in every keymap (see [Keys](keys.md#editing-keys-in-text-fields)). `:field-keys` cycles and saves it. |
| `thread_order` | `"oldest"` | How the reader lists a thread's older messages under the newest one: `"oldest"` first or `"newest"` first. `:thread-order` in the palette toggles it and saves it. |
| `layout` | `"auto"` | Only `"auto"` works in this version; `"split"` and `"stack"` are accepted and act as `"auto"`. |
| `editor` | unset | The command that writes mail, split like a shell command line (no shell runs), e.g. `"nvim"` or `"code --wait"`. Unset: `$VISUAL`, then `$EDITOR`, then `vi`. Read when the mail screen starts. |
| `bulk_confirm` | `50` | How many threads an archive, star, mark-read or trash without asking (`list.trash_now`) may cover before Rail asks first; `0` never asks. `d` always asks. |
| `attachment_dir` | unset | Where `s`, `S` and `w` save attachments. Unset is your download directory (the `XDG_DOWNLOAD_DIR` entry of `~/.config/user-dirs.dirs` on Linux, else `~/Downloads`). `~` is expanded, and refused when there is no `$HOME` to expand it to; a relative path is refused, as is one inside Rail's own attachment cache (`attachments` in the cache directory, reached directly, through a symlink or through `..`), whose files Rail sweeps a day after it writes them. |
| `saved_search_counts` | `true` | Unread counts beside each saved search in the sidebar. `false` leaves them out and skips the count queries, the one thing saved searches cost on a large mailbox. `:search-counts` toggles it and saves it. |
| `rich_html` | `true` | The reader draws HTML mail's bold, italic, quoted and linked text in those styles. `false` shows the same text plain. `:rich-html` toggles it and saves it. |
| `images` | `true` | With `rich_html`, the reader draws the pictures an HTML message carries inline, through the terminal's image protocol (kitty, iTerm2 or sixel) or as coloured half blocks elsewhere. Rail asks the terminal which it speaks as it starts, waiting up to 2 s for one that does not answer; with `TERM` unset, empty or `dumb` it does not ask and draws half blocks. `false` leaves them out. `:images` toggles it and saves it. |
| `remote_images` | `false` | With `images`, the reader fetches every HTML message's remote pictures, which tells their senders you opened the mail. `false` holds them back until `i` loads one thread's. `:remote-images` toggles it and saves it. |
| `remote_images_from` | `[]` | Senders whose remote pictures the reader always fetches: an address such as `"jira@example.com"`, or `"@example.com"` for every address at exactly that domain (not its subdomains). Only for mail whose From line the receiving server vouched for (DMARC, or DKIM aligned with the sender's domain), since a forged message can carry the address too. `I` on a message asks whether to add its sender or their domain, or removes the one that matched, and saves the list. |
| `remote_images_ipv6` | `true` | Whether the daemon may fetch remote pictures over IPv6. `false` refuses every IPv6 address, for an IPv6-only network whose NAT64 prefix (which looks public) could reach private IPv4 space. Read at each picture, so a change applies without a restart. |

Most of these have a palette command that changes and saves them; see
[Settings in the palette](reading.md#settings-in-the-palette).

## `[keys]`

`[keys]` changes the key of any action. The key names, the presets and every action id are in
[Keys](keys.md).

## `[[saved_searches]]`

One table per saved search, in the order the sidebar lists them.

- `S` on the mail screen writes them, and so do `rename-search` and `remove-search`; your
  comments, formatting and other tables survive each write.
- A rename edits the table where it stands, so its row keeps its place.
- A rename changes only the `name` line: the `query` stays exactly as the file has it, so a query
  you edited by hand while Rail was open is kept.

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | 1 to 40 ASCII letters, digits, `-` or `_`; unique among saved searches, compared ignoring case. It is what the sidebar, the palette and the header show. |
| `query` | yes | Filter text, exactly as you would type it after `/`; not empty, at most 1024 characters. |

```toml
[[saved_searches]]
name = "needs-reply"
query = "from:dana is:unread"

[[saved_searches]]
name = "big-attachments"
query = "has:attachment"
```

### Rules

- Rail keeps at most 20 saved searches.
- A name with a space or an accent in it, a duplicate name, an empty or over-long query or a 21st
  table makes `config.toml` invalid. `rail doctor` says which entry is wrong: by name and
  position, never by quoting the query, which is yours.
- A query that narrows nothing (only characters the filter ignores, such as `* ( )`) lists no
  threads and counts none, rather than every thread of every account.

### Going back to an older Rail

A Rail older than saved searches cannot read a `config.toml` that holds `[[saved_searches]]`
tables or `saved_search_counts` in `[general]`: it reports the config invalid. To go back to an
older version, remove them first.

How saved searches behave on screen: [Saved searches](reading.md#saved-searches).

## `[[link_actions]]`

Mail from an issue tracker carries the record's id in its subject (`(#12345)`, `[PROJ-123]`,
`#4711`), so the way to the record is a substitution, not a search.

One table per rule. `g t` (`list.open_link`, in the list and the reader) runs every rule over the
subject of the thread under the cursor.

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | 1 to 40 ASCII letters, digits, `-` or `_`; unique among link actions, compared ignoring case. |
| `match` | yes | A regular expression over the subject. Its capture groups fill the template. |
| `url` | yes | An `http` or `https` template whose `{1}`, `{2}`, … are `match`'s groups, numbered from 1. |

```toml
[[link_actions]]
name  = "Ticket"
match = '\(#(\d+)\)'
url   = "https://tracker.example.com/ticket/{1}"

[[link_actions]]
name  = "Merge-request"
match = '!(\d+)'
url   = "https://gitlab.example.com/group/project/-/merge_requests/{1}"
```

### What `g t` offers

- Every match of every rule is offered, so a subject carrying two ids reaches either.
- One match opens after the same confirmation as any other link.
- Several are listed by number with the rule that found each and the address it opens. On a
  narrow terminal an address loses its middle, so the id at its end stays visible.
- One address is one choice, whether a subject names the same record twice or two rules read it
  the same way.
- A subject no rule matches is a notice, not an error.
- Rail keeps at most 20 link actions.

### How captures are made safe

The subject is the sender's text, so a capture is never trusted.

- Each capture is percent-encoded before it reaches the template, which leaves it no `/`, `?`, `#`
  or `@` to steer the address with.
- Encoding cannot defuse a dot segment, though: a parser reads `%2e%2e` back as `..`. So a capture
  of `.` or `..` drops the match rather than climbing out of the path the rule chose.
- A template whose own path holds such a segment, written with dots or `%2e`, is refused outright,
  since an empty capture beside it would climb just the same.
- The address Rail then builds goes through the same confirmation as a link in a message, with
  the warnings described under [Reading mail](reading.md#links).

Everything before the path (the scheme, any credentials, the host and the port) comes from this
file and from nowhere else. To settle that, Rail fills the template twice with different values and
reads both with the parser that later opens the link, refusing the rule unless the two agree.

Reading the template any other way would let the check disagree with what the address turns out to
mean: `https:///{1}` and `https://\/{1}` both look like they name a host, and a parser hands the
host to the capture in each.

### Rules that make the config invalid

`config.toml` is invalid, and `rail doctor` names the rule, for:

- a name that breaks the charset, or a duplicate name;
- a pattern that does not compile;
- a template that is not `http`/`https`;
- a template a match could steer before the path;
- a template whose path holds a `.` or `..` segment;
- a template with no `{n}` at all, or one naming a group the pattern does not have.

## `[[accounts]]`

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | Short name used in commands and output; unique among accounts. |
| `kind` | yes | `"gmail"` or `"m365"`. |
| `address` | yes | The mailbox address. Must contain `@`, at most 254 characters. |
| `client_id` | yes | Your OAuth client id (Google) or Application (client) ID (Microsoft). |
| `client_secret` | Gmail | The desktop client's secret, or an `op://vault/item/field` reference. |
| `tenant` | no | Microsoft tenant ID or domain (default `common`). |
| `color` | no | Account colour in the UI. |
| `label` | no | The account's name on screen: the sidebar, the header, `Write from`, the account states and the palette (at most 40 characters; `name` stays what commands, `--account` and `account:` use). `rail doctor` warns when two accounts show the same one. |
| `history_days`, `body_cache_days` | no | Per-account overrides of the `[general]` values. |
| `ai_send` | no | `"confirm"` (default), `"drafts"` or `"client"`; see [below](#ai_send-what-an-ai-agent-may-do-with-your-mail). |
| `signature` | no | A text file whose contents go under `-- ` at the end of every new message, reply and forward from this account (`~` is your home directory; trailing blank lines are dropped; at most 4 KiB are used). Read when the mail screen starts; a file that cannot be read is noticed when you start writing, and the message goes without it. |
| `from_name` | no | The name shown before `address` in `From:` of mail Rail sends (spaces allowed, at most 200 characters). |

- `address`, `color` and `tenant` must not contain spaces, control characters or Unicode format
  characters (such as U+202E). Such a value is refused by `rail account add` and fails to load
  from a hand-edited file.
- `from_name` may contain ordinary spaces but no other whitespace, control or format characters.

How to get the `client_id` and the rest: [Account setup](accounts.md).

## Sync settings

```toml
[general]
poll_interval = "60s"
history_days = 0        # whole mailbox
body_cache_days = 30

[[accounts]]
name = "work"
# ...
history_days = 90       # this account keeps only the last 90 days
body_cache_days = 7
```

### How the settings interact

- A `body_cache_days` larger than a non-zero `history_days` is treated as `history_days`.
- Shrinking `history_days` removes local copies of older mail on the next pass (mail on the server
  is never touched).
- Growing it fetches further back, at most 2,000 messages a pass, while new mail keeps arriving
  within one `poll_interval`.
- Starred mail and threads with recent mail are never pruned.

### Gmail

- A first Gmail sync is paced to stay under Gmail's per-user quota, about 250 messages a minute:
  100,000 messages take about seven hours.
- Newest mail is fetched first.
- Mail that arrives during the first sync appears once it finishes; none is missed.
- An interrupted sync resumes where it stopped.
- If Gmail's history for the account expires (after a long time offline), Rail does a full
  resync, which reads every message again.

### Microsoft 365

- A Microsoft 365 account is paced to Microsoft's limits, at most 900 requests a minute, and reads
  folder by folder.
- With a non-zero `history_days`, Microsoft returns at most 5,000 messages per folder, and growing
  `history_days` reads every folder again.

See [Microsoft 365 mail](accounts.md#microsoft-365-mail).

## `ai_send`: what an AI agent may do with your mail

```toml
[[accounts]]
name = "work"
# ...
ai_send = "confirm"
```

| Value | Meaning |
|---|---|
| `confirm` (default) | Rail shows you the recipients, subject and start of the body in your MCP client's confirmation dialog and sends only on an explicit yes. A client without that dialog gets drafts-only behaviour. |
| `drafts` | The agent may create and edit drafts, never send. |
| `client` | Rail relies on your MCP client's own per-call approval prompt. The weakest option. |

The policy will cover sending, replying, forwarding and deleting.

The operations that save, send and discard a draft, save one to the account's Drafts and import
one from there exist (`draft.save`, `draft.send`, `draft.discard`, `draft.push`, `draft.import`),
and Rail's own terminal uses them. They are not offered over MCP: no agent tool for mail exists
yet, so today the key is only recorded and reported (`rail doctor`, `rail_status`,
`rail_accounts_list`).

## Tokens in 1Password

With the [1Password CLI](https://developer.1password.com/docs/cli/) (`op`, version 2) installed and
signed in:

```toml
[general]
secrets = "1password"

[onepassword]
vault = "Rail"                    # required
# account = "my.1password.com"    # when several 1Password accounts are signed in
# op_path = "/usr/bin/op"         # default: the first op on PATH
```

Rail keeps one item per mail account, titled `Rail: <address>` and tagged `rail`, with the tokens in
concealed fields. Values reach `op` only on its standard input, never as command-line arguments.

- **Turn on the 1Password app's CLI integration** (Settings → Developer → Integrate with 1Password
  CLI). The daemon can then refresh tokens however it was started. A session from `op signin`
  lives only in the shell that ran it and expires after about 30 minutes without use.
- **Moving tokens.** `rail secrets set 1password --vault <vault>` copies each account's tokens,
  reads them back to verify, updates `config.toml`, and only then deletes the old copies. A
  failure part way leaves the old copies in charge. The same command moves them back to `keyring`
  or `file`.
- **Client secret in 1Password.** Give `client_secret = "op://Rail/Google client/client_secret"`
  (or pass an `op://` reference to `--client-secret`), or let Rail store it with
  `rail account add ... --client-secret <secret> --store-secret-in-1password`. Rail reads it with
  `op read` only when a login needs it and never caches or logs it. This works with any token
  backend.
- **`op_path`** is only ever read from `config.toml`. Rail refuses it when it is relative, not an
  executable, owned by another user, or writable by anyone but its owner, or when a directory
  above it is. A Homebrew-on-Linux `op` usually fails this check; prefer 1Password's own package
  (for example `/usr/bin/op`).

## File locations

| What | Linux | macOS |
|---|---|---|
| Config | `~/.config/rail/config.toml` | `~/Library/Application Support/rail/config.toml` |
| Config lock (held while Rail edits the config) | `~/.config/rail/config.lock` | `~/Library/Application Support/rail/config.lock` |
| Presets | `~/.config/rail/presets/*.toml`, `/etc/rail/presets/*.toml` | `~/Library/Application Support/rail/presets/*.toml`, `/etc/rail/presets/*.toml` |
| Store | `~/.local/share/rail/rail.db` | `~/Library/Application Support/rail/rail.db` |
| Token file (`secrets = "file"`) | `~/.local/share/rail/secrets.json` | `~/Library/Application Support/rail/secrets.json` |
| Attachments of imported Gmail drafts | `~/.local/share/rail/draft-files/` | `~/Library/Application Support/rail/draft-files/` |
| Attachments opened from the reader | `~/.cache/rail/attachments/` | `~/Library/Caches/rail/attachments/` |
| Log | `~/.local/state/rail/rail.log` | `~/Library/Application Support/rail/rail.log` |
| Last time typed into the snooze chooser | `~/.local/state/rail/snooze-last` | `~/Library/Application Support/rail/snooze-last` |
| Daemon socket | `$XDG_RUNTIME_DIR/rail/rail.sock` (the state directory when there is no runtime directory) | `~/Library/Application Support/rail/rail.sock` |
| Login unit | `~/.config/systemd/user/rail.service` (under `$XDG_CONFIG_HOME` when set) | `~/Library/LaunchAgents/io.github.vitosans.rail.plist` |

Override any location with these variables:

| Variable | Directory |
|---|---|
| `RAIL_CONFIG_DIR` | Config |
| `RAIL_DATA_DIR` | Data (the store, the token file, draft files) |
| `RAIL_CACHE_DIR` | Cache |
| `RAIL_STATE_DIR` | State |
| `RAIL_RUNTIME_DIR` | The directory that holds the daemon's socket |

Give each setup its own `RAIL_RUNTIME_DIR` to run two side by side.

## The background daemon

Every operation (logins, saves, sync, search, `rail doctor`) runs in one background process, so two
commands never race on the store, the config or a login. The TUI's own config saves (`:theme`,
`:time`, `:hints` and the like) take the same config lock, so no edit is lost.

### Starting and stopping

- You never start it yourself. The first command that needs it starts `rail daemon run` in the
  background, or your login does after `rail daemon install`.
- `rail daemon install` writes a systemd user unit (Linux) or a launchd agent (macOS) so the
  daemon starts at login. Without it, the daemon starts on demand whenever a `rail` command needs
  it.
- While any account is configured it keeps running and syncs; with none it exits after ten
  minutes idle.
- It also puts snoozed threads back at their time, so a snooze comes back on time while the daemon
  runs (from your login, after `rail daemon install`). One whose time passed while no daemon ran
  comes back as soon as one starts.
- A login you begin keeps waiting in the daemon until you finish it, cancel it, or 15 minutes
  pass, and opens the browser on the display of the command that began it.

### Its environment

The daemon keeps the environment of the command that started it (notably `PATH` and any
`OP_SESSION_*`). Run `rail daemon stop` to have the next command start one with your current
environment.

A command whose `RAIL_*_DIR` settings differ from the running daemon's is refused with
"a Rail daemon is already running with different directories".

### Upgrades

A client replaces a daemon from an older version *or an earlier build of the same version*, so
reinstalling `rail` while a daemon runs is enough.

- Each build carries the time `rail-core` was last built. A `SOURCE_DATE_EPOCH` exported in the
  build environment replaces that time, so exporting it permanently makes every build look alike.
- A program that is itself older than the running daemon, such as a long-lived `rail mcp` after an
  upgrade or rebuild, never stops its successor: it says "Rail was upgraded; restart this program".
  After a rebuild the hint adds that `rail daemon stop` lets this program start a daemon of its
  own build.
- When an upgrade changes the store, the new daemon migrates it the first time it starts
  (rebuilding the search index can take a moment on a large mailbox); until then the mail screen
  says the store is not set up.

### The login unit

The login unit written by `rail daemon install`:

- runs the exact `rail` binary you installed it with (install again after moving it);
- pins the `RAIL_*` directories your shell resolved at install time;
- restarts the daemon if it fails, at most five times in five minutes, and not after a clean
  `rail daemon stop`;
- sets `PATH` to the system directories plus the directory of `op_path` if you set one;
- on Linux, unsets display, browser, `RUST_LOG` and 1Password variables inherited from the user
  service manager.

`systemctl --user show-environment` lists what your user manager can still pass in.
