<h1 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/rail-dark.png">
    <source media="(prefers-color-scheme: light)" srcset="assets/brand/rail-light.png">
    <img alt="Rail. Modern email. Native to your terminal." src="assets/brand/rail-light.png" width="560">
  </picture>
</h1>

[![ci](https://github.com/vitosans/rail/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/vitosans/rail/actions/workflows/ci.yml?query=branch%3Amain)
[![CodeQL](https://github.com/vitosans/rail/actions/workflows/codeql.yml/badge.svg?branch=main)](https://github.com/vitosans/rail/actions/workflows/codeql.yml?query=branch%3Amain)
[![coverage](https://raw.githubusercontent.com/vitosans/rail/badges/coverage.svg)](https://github.com/vitosans/rail/actions/workflows/ci.yml?query=branch%3Amain)
[![version](https://img.shields.io/github/v/tag/vitosans/rail?sort=semver&filter=v%2A&label=version)](https://github.com/vitosans/rail/tags)
[![rust](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](Cargo.toml)
[![platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS-blue.svg)](#install)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

Rail is a terminal mail client for Gmail and Microsoft 365, written in Rust. It keeps a local copy
of your mail in SQLite, synced by a small background daemon, so every list, search and open is a
local query and the interface never waits on the network. It is built for people who live in a
terminal and want something in the spirit of mutt with threads, a unified inbox across accounts,
readable HTML and a command palette. It also speaks MCP, so an AI coding agent can help you set it
up without ever seeing your secrets.

<p align="center">
  <img alt="The mail screen: the sidebar, the unified inbox and an open thread side by side, with the hint bar below" src="assets/screenshots/mail.jpg" width="100%">
</p>
<p align="center"><sub>The mail screen at 150 columns: the sidebar, the unified inbox and an open thread, with the keys for the pane you are in along the bottom. The accounts and the mail in these pictures are invented.</sub></p>

<p align="center">
  <img alt="A 24-second tour: opening a thread and turning its messages round with :thread-order, paging the sidebar, turning the key hints off with :hints, and previewing themes as you type in :theme" src="assets/screenshots/tour.gif" width="100%">
</p>
<p align="center"><sub>A short tour: open a thread and turn its older messages round with <code>:thread-order</code>, page the sidebar with <code>Ctrl-d</code>, turn the key hints off with <code>:hints</code>, and preview themes as you type in <code>:theme</code> (<code>Esc</code> puts yours back).</sub></p>

<table>
  <tr>
    <td width="50%" valign="top" align="center">
      <img alt="The thread list" src="assets/screenshots/list.jpg"><br>
      <sub>The thread list: date groups, an unread dot in the account's colour, the account's badge, the people, stars and attachments, the message count after the subject and the time at the edge.</sub>
    </td>
    <td width="50%" valign="top" align="center">
      <img alt="A thread in the reader" src="assets/screenshots/thread.png"><br>
      <sub>A thread: the title block, the newest message open with its quoted lines folded, and the older messages as numbered cards. <code>n</code> and <code>p</code> move between messages, <code>Tab</code> opens or folds one.</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top" align="center">
      <img alt="A message with its headers shown" src="assets/screenshots/message-headers.png"><br>
      <sub>A message after <code>h</code>: the From, To and Date lines above the body. The title block shows names only, so the addresses stay out of the way until you ask.</sub>
    </td>
    <td width="50%" valign="top" align="center">
      <img alt="The command palette" src="assets/screenshots/palette.png"><br>
      <sub>The command palette on <code>:</code>: every command with its key, narrowed as you type. Themes, mailboxes and the settings toggles live here too.</sub>
    </td>
  </tr>
  <tr>
    <td colspan="2" valign="top" align="center">
      <img alt="The sidebar" src="assets/screenshots/sidebar.png"><br>
      <sub>The sidebar: the merged mailboxes across every account, then each account with its badge, its system folders and <code>More folders</code> for the rest, folded and unfolded with <code>Space</code>.</sub>
    </td>
  </tr>
</table>

> **Rail is pre-1.0.** Account setup, background sync, search, reading mail, acting on threads
> (archive, trash, star, read, move) and writing mail (new messages, replies and forwards, with
> undo send, and drafts saved to the account's Drafts) work today for Gmail and Microsoft 365.
> Labels are Gmail's; on Microsoft 365 `m` moves to a folder instead.

## Contents

- [Status](#status)
- [Install](#install)
- [Quick start](#quick-start)
- [Reading mail](#reading-mail)
- [Doing things to mail](#doing-things-to-mail)
- [Writing mail](#writing-mail)
- [Account setup](#account-setup)
- [Using Rail from an AI agent (MCP)](#using-rail-from-an-ai-agent-mcp)
- [Security and privacy](#security-and-privacy)
- [Configuration](#configuration)
- [Commands](#commands)
- [Troubleshooting](#troubleshooting)
- [Contributing](#contributing)
- [License](#license)

## Status

**Works today**

- Setup wizard (`rail setup`) and a flag form for scripts (`rail account add`), including
  organisation presets and Microsoft admin consent.
- OAuth login for Gmail / Google Workspace and Microsoft 365; tokens kept in the OS keyring, a
  private file, or 1Password.
- Gmail and Microsoft 365 sync into the local store: labels or folders, message envelopes, and
  bodies of recent threads, kept current by a background daemon that can start at login
  (`rail daemon install`).
- Microsoft 365 over Microsoft Graph: every mail folder, threads by conversation, read and flag
  (star) state, new mail within one poll interval, server search, marking read on open,
  archive, trash, star, read or unread and move to a folder, with undo, and writing: new
  messages, replies in the Outlook conversation, forwards, attachments up to 25 MiB and drafts in
  Outlook's Drafts. `l` answers `Outlook uses folders, not labels: press m to move.` See
  [Microsoft 365 mail](#microsoft-365-mail).
- `rail search` over the local store and, through each account's server (Gmail's search,
  Microsoft Graph's `$search`), the mailbox itself.
- Reading mail in the terminal: a unified inbox and per-account and per-label lists over the local
  store, a sidebar of merged mailboxes and each account's folders as a folding tree, a reader that
  opens on the newest message with the older ones as cards, folded quotes and numbered
  (clickable) links, HTML mail drawn with its styles and pictures (remote pictures only when you
  ask), filter as you type, a command palette, keys you can
  change in `config.toml`, and `o` to open a message's HTML in the browser. See
  [Reading mail](#reading-mail).
- `[[link_actions]]`: a pattern over a thread's subject and the address it stands for, so `g t`
  reaches the ticket or issue the thread is about. See [`[[link_actions]]`](#link_actions).
- Archive, trash, star and mark read or unread, alone or as a selection, with undo, on Gmail and
  Microsoft 365. See [Doing things to mail](#doing-things-to-mail), including labelling (`l`, Gmail)
  and moving (`m`).
- Writing mail: a new message, a reply, a reply to everyone or a forward, written in your own
  editor, reviewed in Rail and sent through the account (Gmail or Microsoft 365) with a ten second
  undo; drafts kept on this machine or saved to the account's Drafts, and a Drafts folder that
  lists both. See
  [Writing mail](#writing-mail).
- `rail doctor`, a single health check for config, store, tokens, sync and the daemon.
- An MCP server (`rail mcp`) for setting up and inspecting Rail from an AI agent.

**Coming**

- Next: agent tools for drafts and sends, the last piece of mail work, governed by the per-account `ai_send` policy.
- Later: search operators, layout options.

**Pre-1.0 caveats.** Config keys, command output and the store's schema may change between
releases. The store migrates forward on its own; there is no downgrade path. Rail is tested on
Linux and macOS only.

## Install

Binaries are available on the [GitHub releases page](https://github.com/vitosans/rail/releases)
with SHA-256 checksums: Linux x86_64 and ARM64 (glibc 2.36 or newer) and macOS on Apple Silicon
and Intel (macOS 11 or newer). Up to v0.2.3 only Linux x86_64 was published. Extract the archive
and put `rail` in a directory on your `PATH`. GitHub Actions builds each archive and attests where
it came from; check one with `gh attestation verify <archive> --repo vitosans/rail`. Build from
source on other systems.

Rail builds from source with Cargo. You need Rust 1.88 or newer and a C compiler (SQLite is
bundled and compiled in). Supported platforms are Linux and macOS.

```sh
git clone https://github.com/vitosans/rail
cd rail
cargo install --locked --path crates/rail
```

This puts `rail` in `~/.cargo/bin`. To upgrade, pull and run the same command; the first command
of the new version replaces any older daemon still running, including one from an earlier build
of the same version. When an upgrade changes the store, the new daemon migrates it the first time
it starts (rebuilding the search index can take a moment on a large mailbox); until then the mail
screen says the store is not set up.

## Quick start

```sh
rail                       # read your mail
rail setup                 # add an account: guided steps, then a browser login
rail sync                  # fill the local store and watch progress
rail search invoice        # one line per matching thread
rail doctor                # check that everything is in order
rail daemon install        # keep syncing after you log out and back in
```

1. **`rail`** opens the mail screen (see [Reading mail](#reading-mail)).
2. **`rail setup`** opens the setup wizard (running `rail` with no config does the same). Choose a
   preset your organisation gave you, or set up your own app: the wizard shows the portal steps
   from [Account setup](#account-setup), opens the portal with `o`, asks for the values it gave
   you, and logs you in through the browser. It needs an interactive terminal; in scripts use
   [`rail account add` with flags](#commands).
3. **`rail sync`** wakes the background sync and shows progress (counts only, never mail) until
   each account is caught up. Newest mail arrives first, so recent mail is searchable within
   seconds, but a first sync of a large mailbox takes hours (about 250 messages a minute, under
   Gmail's quota). Set [`history_days`](#sync-settings) to sync only recent mail. Ctrl-C stops
   watching, not the sync.
4. **`rail search <query>`** prints date, account, sender and subject for each matching thread,
   newest first. Gmail's search syntax works for Gmail accounts; Microsoft 365 takes `from:`,
   `subject:`, `to:`, `body:` and plain words (see [Microsoft 365 mail](#microsoft-365-mail)).
   When a server is busy or refuses the connection (`not searched on Gmail` or
   `not searched on Microsoft 365` says which), it is asked once more after a short wait; the
   local results still print.
5. **`rail doctor`** reports on the config, store, token backend, each account's tokens and sync,
   the daemon and the login unit.
6. **`rail daemon install`** writes a systemd user unit (Linux) or a launchd agent (macOS) so the
   daemon starts at login. Without it, the daemon starts on demand whenever a `rail` command needs
   it and keeps running while any account is configured.

### Keys

| Key | Action |
|---|---|
| `↑` / `↓` | Move within a list. |
| `Tab` / `Shift-Tab` | Change focus between fields. |
| `Enter` | Choose or continue. |
| `Space` | Tick or untick the checkbox. |
| `Esc` | Go back one step. |
| `F1` | Open the provider portal, or the preset's help link. |
| `o` | On the Instructions step, open the provider portal in your browser. |
| `r` | Retry when an error is shown. |
| `c` | Copy the admin-consent URL shown with an error, or the login URL on the Login step (OSC 52; some terminals ignore it, so select the URL on screen if nothing was copied). |
| `q` | Quit, except while typing in a text field. |
| `Ctrl-C` | Quit from anywhere. While an account is being saved, it waits for the save to finish. |

These are the wizard's default keys, the `wizard.*` actions in the table below; each can be changed.

The wizard exits 0 if at least one account was saved and 1 with `setup cancelled; nothing was
saved` otherwise. Quitting while the browser login is pending cancels that login. An account whose
save had already started is always finished, never left half-written; Rail then says so and exits
0. If you quit after an admin-consent error, Rail prints the admin-consent URL once the terminal is
back so you can forward it to an administrator. When stdout is not a terminal, every way of opening
the wizard prints a hint and exits 2.

After a browser login, Rail verifies that the signed-in mailbox matches the account's address
before saving credentials or adding the account. Selecting a different mailbox fails the login
and preserves existing credentials. Address comparisons ignore case. For Gmail, use the primary
mailbox address (consumer Gmail dot, plus-tag and `googlemail.com` variants also match); Google
Workspace custom aliases require the primary address. Microsoft 365 also accepts the signed-in
user's principal name and SMTP aliases returned by Graph. Verification uses the existing OAuth
permissions; if the profile cannot be read, finish the login again once the error is resolved.

Every key Rail reads is an *action* with an id and a default key. Change one in `config.toml`:

```toml
[keys]
"list.filter" = "/"
"reader.open_browser" = "O"
"global.palette" = ":"
```

A key name is a character (`a`, `O`, `/`), a named key (`enter`, `esc`, `tab`, `backtab`,
`space`, `up`, `down`, `left`, `right`, `pageup`, `pagedown`, `home`, `end`, `f1` to `f12`),
either with `ctrl-`, `alt-` or `shift-` in front (`ctrl-r`, `shift-tab`, `shift-down`; `shift-`
goes on a letter, `tab`, `up` or `down`), or two keys with a
space between them for a sequence (`g x`). A key you set replaces that action's default keys.
Both `rail` and `rail setup` refuse to start, naming the file and line and exiting 1, when `[keys]`
names an action it does not know, a key it cannot read, `Ctrl-C` (which always quits), or a key
another action of the same view already uses (a key that starts another action's sequence
counts). They check before the terminal changes. A view's own key wins over a global one: `q`
closes the reader but quits from the list. The defaults count too: `J`, `K`, `shift-down` and
`shift-up` extend the selection in the list, so a `[keys]` entry that binds one of them to another
list action (`"list.down" = "J"`, say) stops Rail from starting with that error until you change
the entry or move `list.extend_down` / `list.extend_up` to other keys. Likewise `h` snoozes in the
list and the reader, so the reader's headers moved to `H`: an older `"reader.headers" = "h"` entry
stops Rail from starting with `[keys] "list.snooze" (h) and "reader.headers" (h) cannot share a
key in the reader view` until you remove it or bind `list.snooze` to another key.

The `global.*`, `list.*`, `sidebar.*`, `reader.*` and `compose.*` actions below take effect in the
mail screen and the `wizard.*` ones in the setup wizard. In the mail screen `?` lists the keys of the view you
are in, with `*` after the ones set in `config.toml`.

| Action | Works in | Default keys | What it does |
|---|---|---|---|
| `global.quit` | global | `q` | Quit Rail. |
| `global.palette` | global | `:` | Open the command palette. |
| `global.help` | global | `?` | Show the keys for this view. |
| `global.undo` | global | `z` | Undo the last action. |
| `list.open` | list | `enter` | Open the thread. |
| `list.filter` | list | `/` | Filter the list as you type. |
| `list.save_search` | list | `S` | Save the filter as a search. |
| `list.sidebar` | list, sidebar | `b` | Show or hide the sidebar. |
| `list.account_1` | list, sidebar | `1` | Go to account 1's inbox. |
| `list.account_2` | list, sidebar | `2` | Go to account 2's inbox. |
| `list.account_3` | list, sidebar | `3` | Go to account 3's inbox. |
| `list.account_4` | list, sidebar | `4` | Go to account 4's inbox. |
| `list.account_5` | list, sidebar | `5` | Go to account 5's inbox. |
| `list.account_6` | list, sidebar | `6` | Go to account 6's inbox. |
| `list.account_7` | list, sidebar | `7` | Go to account 7's inbox. |
| `list.account_8` | list, sidebar | `8` | Go to account 8's inbox. |
| `list.account_9` | list, sidebar | `9` | Go to account 9's inbox. |
| `list.unified` | list, sidebar | `0` | Go to the unified inbox. |
| `list.refresh` | list, sidebar, reader | `ctrl-r` | Sync now; reconnect to the daemon. |
| `list.down` | list | `j` `down` | Next thread. |
| `list.up` | list | `k` `up` | Previous thread. |
| `list.page_down` | list | `pagedown` `ctrl-d` | One page down. |
| `list.page_up` | list | `pageup` `ctrl-u` | One page up. |
| `list.top` | list | `home` `g g` | Newest thread. |
| `list.bottom` | list | `end` `G` | Oldest thread. |
| `list.archive` | list, reader | `e` | Archive the thread or the selection. |
| `list.trash` | list, reader | `d` | Move to the trash (asks first). |
| `list.star` | list, reader | `s` | Star or unstar. |
| `list.read` | list, reader | `u` | Mark read or unread. |
| `list.labels` | list, reader | `l` | Add or remove labels (Gmail). |
| `list.move` | list, reader | `m` | Move to a label (Gmail) or a folder (Microsoft 365). |
| `list.snooze` | list, reader | `h` | Snooze until later, or return a snoozed thread ([Snooze](#snooze)). |
| `list.compose` | list, reader | `c` | Write a new message. |
| `list.open_link` | list, reader | `g t` | Open the link the thread's subject names ([`[[link_actions]]`](#link_actions)). |
| `list.reply` | list, reader | `r` | Reply to the latest message. |
| `list.reply_all` | list, reader | `R` | Reply to everyone. |
| `list.forward` | list, reader | `f` | Forward the latest message. |
| `list.select` | list | `space` | Select or unselect the thread. |
| `list.clear_selection` | list | `esc` | Clear the selection. |
| `list.select_all` | list | `* a` | Select every thread this mailbox and filter match. |
| `list.select_sender` | list | `* f` | Select every thread from this sender. |
| `list.select_range` | list | `v` | Select a range of threads. |
| `list.extend_down` | list | `J` `shift-down` | Extend the selection down. |
| `list.extend_up` | list | `K` `shift-up` | Extend the selection up. |
| `sidebar.focus` | list, sidebar | `tab` | Move between the sidebar and the list. |
| `sidebar.down` | sidebar | `j` `down` | Next mailbox. |
| `sidebar.up` | sidebar | `k` `up` | Previous mailbox. |
| `sidebar.page_down` | sidebar | `pagedown` `ctrl-d` | One page down. |
| `sidebar.page_up` | sidebar | `pageup` `ctrl-u` | One page up. |
| `sidebar.open` | sidebar | `enter` | Show this mailbox. |
| `sidebar.fold` | sidebar | `space` `left` `right` | Fold or unfold this account or folder. |
| `sidebar.rename_search` | sidebar | `r` | Rename this saved search. |
| `sidebar.remove_search` | sidebar | `d` | Remove this saved search (asks first). |
| `reader.next` | reader | `n` | Next message. |
| `reader.prev` | reader | `p` | Previous message. |
| `reader.expand` | reader | `tab` | Expand or collapse the message. |
| `reader.unfold` | reader | `enter` | Show the quoted lines, or open the message. |
| `reader.headers` | reader | `H` | Show or hide the headers. |
| `reader.open_browser` | reader | `o` | Open the message in the browser. |
| `reader.attachments` | reader | `a` | List the attachments. |
| `reader.save_attachments` | reader | `w` | Save this message's attachments. |
| `reader.load_images` | reader | `i` | Load this thread's remote pictures. |
| `reader.trust_images` | reader | `I` | Always load this sender's remote pictures, or stop. |
| `reader.follow_link` | reader | `g x` | Open a link. |
| `reader.back` | reader | `q` `esc` | Close the thread. |
| `reader.down` | reader | `j` `down` | Next line. |
| `reader.up` | reader | `k` `up` | Previous line. |
| `reader.page_down` | reader | `pagedown` `space` | One page down. |
| `reader.page_up` | reader | `pageup` | One page up. |
| `reader.top` | reader | `home` `g g` | First line. |
| `reader.bottom` | reader | `end` `G` | Last line. |
| `compose.send` | compose | `y` | Send. |
| `compose.push` | compose | `s` | Save to Drafts (Gmail or Microsoft 365). |
| `compose.edit` | compose | `e` | Edit in your editor. |
| `compose.headers` | compose | `t` | Change To, Cc, Bcc or Subject. |
| `compose.attach` | compose | `a` | Attach a file. |
| `compose.remove` | compose | `x` | Remove the highlighted attachment. |
| `compose.close` | compose | `q` `esc` | Close and keep the draft. |
| `compose.discard` | compose | `d` | Discard the draft (asks first). |
| `compose.down` | compose | `j` `down` | Next line. |
| `compose.up` | compose | `k` `up` | Previous line. |
| `compose.page_down` | compose | `pagedown` `space` | One page down. |
| `compose.page_up` | compose | `pageup` | One page up. |
| `wizard.back` | wizard | `esc` | Go back one step. |
| `wizard.portal` | wizard | `f1` | Open the provider portal or the preset's help. |
| `wizard.next_field` | wizard | `tab` | Next field. |
| `wizard.prev_field` | wizard | `backtab` | Previous field. |
| `wizard.next` | wizard | `enter` | Choose or continue. |
| `wizard.up` | wizard | `up` `k` | Move up. |
| `wizard.down` | wizard | `down` `j` | Move down. |
| `wizard.page_up` | wizard | `pageup` | Scroll one page up. |
| `wizard.page_down` | wizard | `pagedown` | Scroll one page down. |
| `wizard.top` | wizard | `home` | Scroll to the top. |
| `wizard.bottom` | wizard | `end` | Scroll to the end. |
| `wizard.open_portal` | wizard | `o` | Open the portal from the instructions. |
| `wizard.toggle` | wizard | `space` | Tick or untick the checkbox. |
| `wizard.retry` | wizard | `r` | Retry after an error. |
| `wizard.copy_url` | wizard | `c` | Copy the URL shown. |

## Reading mail

`rail` with an account configured opens the mail screen. It needs an interactive terminal; in a
pipe it prints a hint and exits 2. It reads the local store at once, and connects to the background
daemon (starting it if needed) for sync progress. If the store is not set up yet, the list says so
and points to `rail sync`; if the daemon cannot be reached, the header says so
(`daemon unreachable (Ctrl-r retries)`) and `Ctrl-r` retries.

The thread list shows the unified inbox, newest first, in date groups: `TODAY`, `YESTERDAY`,
`THIS WEEK` (two to six days ago) and `OLDER`, each name on its own row above its threads; the
group of the top row stays on the first row as you scroll. `PgDn` and `PgUp` (`Ctrl-d`, `Ctrl-u`)
move a page by the threads on screen: the next page starts with the thread after the last one
shown, the page above ends with the thread before the first one shown, and the cursor keeps its
row, or goes to the page's last thread when the page holds fewer. A group's name never sits alone
on the bottom row. A row shows a dot (`●` in the
account's colour while the thread has unread mail, a muted `·` once it is read, `▸` when it is
selected with `Space`), the account's badge in `All inboxes`, `Starred`, `Sent` and `Trash` (its
label's first letter on its colour), the people, `★` for starred and `📎` for attachments, the
subject with the message count after it (`(3)`, never cut), and at the right edge the time:
`14:05` today and yesterday, `Mon 18:20` this week, `Oct 03` earlier this year, `2024-10-03`
before; mail dated after today is under `TODAY` with its date, and a date too far off to read
is under `TODAY` when ahead and `OLDER` when behind. With `time = "12h"` (or `:time`) the times
read `2:05 pm` and `Mon 6:20 pm`, and the time column is two cells wider.
Unread threads are bold.
With `icons = "ascii"` the dots are `*`, `.` and `>`, `★` is `+` and `📎` is `@`.
`1` to `9` show one account's inbox, `0` the unified inbox again. The sidebar (shown at 100 columns
and wider; `b` toggles it, `Tab` moves into it) has two sections. UNIFIED holds `All inboxes`,
`Starred`, `Drafts`, `Sent` and `Trash`, each with every account's mail. ACCOUNTS holds each
account (by its `label`) with its system folders under the provider's names (Inbox, Sent,
Drafts, Spam and Trash on Gmail; Inbox, Sent Items, Drafts, Archive, Junk Email and Deleted Items
on Microsoft 365); every other folder or label (up to 500 per account) is under
`More folders (N)`, as a tree of its `/`-separated path (`Inbox/Infrastructure/Linode`), with
Gmail's categories under `Categories`. `Enter` opens the mailbox under the cursor (an account
opens its inbox; on Microsoft 365 a top-level path named after a system folder, like `Inbox`
above, opens that folder; `More folders` and any other path that is not a folder itself unfold
instead, as does Gmail's `Sent` path of a label `Sent/2024`, which is not inside Sent).
`Space`, `←` or `→` folds or unfolds an account, `More folders` or a path; accounts start
unfolded and the rest folded, for the session. Counts are unread threads (on a folded path,
everything under it), drafts on `Drafts`, and none on `Sent` and `Trash`.
Each account row has its fold arrow (`▾` unfolded, `▸` folded) and a badge, its label's first
letter on the account's colour. Mailboxes have icons (`✉` inbox, `★` Starred, `✎` Drafts, `➤`
Sent, `✕` Trash, `▣` Archive, `⊘` Spam, `📁` `More folders` and a path with folders under it)
and their counts on the right with thousands separators. With `icons = "ascii"` the arrows are
`v` and `>` and the icons `i`, `*`, `d`, `>`, `x`, `a`, `s` and `/`. While the sidebar does not have
focus the current mailbox's row is in the accent colour; the sidebar scrolls to keep it (or, while the
sidebar has focus, the cursor) in view.

The top of the screen is the header: `▍Rail │ All inboxes · 2 accounts · 22,853 unread`
(`Starred · 2 accounts · 12 unread`, `Sent · 2 accounts` in the merged mailboxes; an account's
own Sent, Trash or Drafts shows no unread count, `work · Sent`), the search region
(`│ 🔍 / Search`, `| / Search` with `icons = "ascii"`; the filter shows there once set, and the
Drafts folder has none), and on the right one line for every account's sync: `● Synced 14:47`
when all are caught up (the earliest account's time), else the worst state (`⟳ Syncing work`,
`⟳ Backfill 1,200/5,000`, `⟳ retrying`, `work stopped`, `! 2 failed`, `! work needs login`,
`! work sync failed`, or the daemon's state), then the time of day at the right edge:
`● Synced 14:47 │ 15:02` (`3:02 pm` with `time = "12h"`; `:time` changes it at once).
`clock = false` under `[general]`, or `:clock`, hides the clock. At `split_threshold` (140 columns
by default) and wider the sidebar, the list and the reader are boxed, the focused pane in the accent
colour; with `icons = "ascii"` the boxes are drawn with `+`, `-` and `|`, and an unboxed header
reads `|Rail | All inboxes …`. At `split_threshold` and wider, and at least 12 rows high, the header
has a box of its own too (never in the accent colour), and inside it the row reads
`│ Rail │ All inboxes … ● Synced 14:47 │ 15:02 │`, without the `▍` bar (`| Rail | …` with
`icons = "ascii"`). When the scope and the summary do not both fit, the summary and the clock stay
whole and the scope is cut. While a filter is set or being typed, the search slot keeps at least 24
cells (at 60 columns and wider; less only when the summary itself leaves no room): the scope is
cut first. Under 60 columns the clock and the search region go, and under 40 the summary.
The status bar at the bottom names the keys of the pane you are
in (`Enter open  j/k move  Tab sidebar  r reply  …  ? help`, with the keys your `config.toml` sets)
and shows each account's state on the right. Narrower than `split_threshold` it is one row; at
`split_threshold` and wider (and at least 12 rows high) it takes three rows and each key is drawn in
its own box, with no frame around the bar (`+`, `-` and `|` with `icons = "ascii"`):

```
┌───────┐      ┌─────┐      ┌─────┐         ┌───┐
│ Enter │ open │ j/k │ move │ Tab │ sidebar │ ? │ help        ● work ✓ · ● home ✓
└───────┘      └─────┘      └─────┘         └───┘
```

`:hints` (or `hints = false` under `[general]`) turns the hints off: the bar is one row of notices
and account states at every width, and `?` still lists the keys.

`Enter` opens a thread. At `split_threshold` columns (140 by default) and wider the reader opens
beside the list; narrower, it covers the screen. `q` or `Esc` closes it. The reader starts with
the thread's title block: the subject; the newest message's sender, its time (`04:32` today,
`Thu 08:55` yesterday and this week, `Oct 03` or `2024-10-03` before) and the account's badge
and label (`Dana Reyes  04:32   W  WORK`); its recipients by name and the thread's size
(`To: Vito, Tom  /  4 messages`, or `/  4 messages` when it names none); then a rule. The
newest message is open under it. The older messages follow as cards between rules, oldest first
(`thread_order = "newest"` under `[general]` runs them newest first): a card is the message's
number in the thread in a grey box, its sender (bold while unread, `★` when starred) and its time
at the right edge (`▸   2   Tom` … `Mon 22:11`), over a line of its text. When a long thread's
oldest messages are not shown, `[5 older messages of this thread are not shown]` stands where they
would be. `j`/`k` move the cursor, `n`/`p` jump between the newest message and each card, `Tab` (or
`Enter` on a card, the subject or a preview) opens or closes the message under the cursor, and
`Enter` on `[n quoted lines]` shows the quoted text. `H` (`h` before snooze took it) shows or
hides, for the session, the headers of every open message: `From:`, `To:` and `Cc:` with addresses,
`Date: Fri 10 Oct 2025 12:40`, and `Subject:` when it differs from the thread's. With
`icons = "ascii"` the arrows are `>` and `v` and the rules `-`.

The pictures a message carries itself are drawn as soon as it opens. **Remote pictures**, the ones
an HTML message loads from the web, are held back, because fetching one tells its sender you
opened the mail: the message starts with
`Remote pictures are not shown; i loads them, I always for this sender.`, a remote picture with
alt text shows `[image: …]` in its place, and `i` loads the open thread's remote pictures. `I`
loads them and remembers the sender of the message under the cursor, by address, so their mail
shows its pictures from then on (`remote pictures from jira@example.com: always`, saved as
`remote_images_from` under `[general]`); `I` on their mail again stops that
(`…: ask first`). A From line can be forged, so a trusted sender's mail loads its pictures by
itself only when the receiving server vouched for the sender's domain: Gmail's or Microsoft 365's
own `Authentication-Results` show DMARC passed for it, or a DKIM signature of it (or of a parent
domain) passed. Other mail from that address waits with the hint, as an untrusted sender's does,
and so does mail stored before Rail kept these results, until its body is fetched again; `i`
loads it. `i` loads every remote picture of the thread, a tracking pixel too when the mail does not
declare its size. The daemon fetches them (never the terminal), only over `https` from public
addresses, with no cookies or referrer, at most 5 MiB each and at most eight at once, and keeps
them for a week in Rail's private image cache, which holds at most 512 MiB (the oldest pictures
go first). On an IPv6-only network with NAT64, the gateway's own prefix looks public but can
reach private IPv4 addresses, which Rail cannot tell; `remote_images_ipv6 = false` under
`[general]` fetches remote pictures over IPv4 only. `remote_images = true` under `[general]` or
`:remote-images` loads them in
every thread.

HTML mail is shown as Rail's own rendering of it, the same text Rail stores and searches, with
its bold and headings bold, its italics italic, its quotes dimmed and its links underlined
(`rich_html = false` under `[general]` or `:rich-html` shows that text without the styles), with
its pictures (at the size it asks for; `[image: …]` until they arrive, and for any past the first
32 in an open thread; pictures of a few pixels, such as tracking pixels, left out; mail stored by
a Rail older than 0.4.0 gains its inline pictures the first time it is opened, with one more fetch
of that message; `images = false` or `:images` leaves them all out), and with each link as a
numbered marker such as `[1]`; in terminals that support hyperlinks (iTerm2, WezTerm, kitty, Ghostty, VS Code, Windows
Terminal, Hyper, foot, Alacritty, GNOME Terminal and other VTE terminals; not inside tmux or
screen) the marker is clickable; while a box such as help or a confirmation is open the markers
are plain text. `g x` opens a link: the marker on the cursor line, or the number you type when the
line has none or several. The confirmation shows where the link really goes before the address
(cut if it is long): the host (an international or look-alike host in its ASCII `xn--` form) or,
for `mailto`, the recipients (`To`, `Cc` and `Bcc`, including those added by the link's
parameters): up to five, then a count of the rest. It warns when the address has text before an
`@` that could pass for a host (`https://bank.com@evil.example` goes to `evil.example`), when a
`mailto` link adds recipients, or when a `mailto` link carries a parameter other than `to`, `cc`,
`bcc`, `subject` and `body` (some mail clients have acted on `attach=`); the warning names up to
five such parameters. Rail opens only `http`, `https` and `mailto` links of at most 2,048 bytes.
`a` lists the thread's attachments (name, type, size) with a cursor: `j` and `k` move it, and each
message's rows sit under a `MESSAGE <n>` heading when more than one message carries attachments.
`Enter` or `o` opens the highlighted one with the program your system uses for it — Rail asks once
a session before the first open, and only for the common document and image types it hands over;
anything else is saved instead and says so. Office documents (Word, Excel, PowerPoint and their
OpenDocument kin) and RTF are among them, so the question warns that they can hold macros or links
that run or fetch content when opened. A type is the sender's claim, and your system's opener
may judge a file by its contents instead, so before writing the copy Rail checks that the file
starts the way its type does (`%PDF-` for a PDF, the PNG, JPEG, GIF, WebP, BMP, TIFF or HEIC
signature for an image, a ZIP whose first part is an Office one for `.docx` and its kin (so a
Java archive renamed `.docx` is refused), a ZIP naming its own type first for
`.odt`, `.ods` and `.odp`, the old Office header for `.doc`, `.xls` and `.ppt`, `{\rtf` for RTF).
Plain text, CSV and TSV may be in any 8-bit encoding (UTF-8, or Latin-1 from a spreadsheet), but
not hold a NUL byte, start (past any byte-order marks and white space) with markup, a `#!`
script line or a PDF or PostScript header, or have an HTML, SVG or XML tag start in their first
64 KiB, whatever its case or the white space inside it. A file that fails is not opened — a web page
labelled as a picture would otherwise open in your browser — and is saved instead, with a line
that says so. `s` saves the highlighted attachment, `S` saves every
attachment of its message, and `Esc`, `q` or `a` closes the list. From the reader itself, `w`
saves the cursor message's attachments without opening the list. Saves land in `[general]
attachment_dir` (below), your download directory by default, and never replace a file you have: a
name already there becomes `2-name.pdf`.

`o` opens the message's original HTML in your browser, for a Microsoft 365 account too (the HTML
Rail stored). Rail writes it to a private (0600) file in
the `open` folder of its cache directory (`~/.cache/rail/open/` on Linux; `RAIL_CACHE_DIR`
overrides the cache directory) with scripts, frames and connections blocked, and removes the file
when you close the reader, quit Rail, close the terminal window or stop Rail with `kill`. Files a
crashed session left behind are removed the next time the mail screen starts, once they are a day
old. The first time in a session Rail asks first, because
**remote images, styles, fonts and media in the message load in the browser**, which can tell the
sender you opened it; Rail itself never loads them. A message whose body is not stored yet is
fetched first. A message with no HTML part says so: its text is all there is.

Two limits of `o`. Firefox installed as a snap (the default on Ubuntu) cannot read hidden folders
such as `~/.cache`, so it shows a file-not-found page: set `RAIL_CACHE_DIR` to a folder it can read
(for example `RAIL_CACHE_DIR=$HOME/rail-cache`) or use a browser that is not a snap. And a
`<meta http-equiv="refresh">` tag in the message can still send the page to another address once
it is open; the blocking above cannot stop it, so close the tab if the page moves.

Bodies of recent mail (`body_cache_days`) are already in the store. An older message shows
**fetching…** while the daemon fetches its body from its account's server (Gmail or Microsoft 365);
the rest of the thread stays readable, and the text appears when it lands. If the fetch fails the
message says why.

`/` opens the filter line in the header's search slot and filters the list as you type. The
slot is not drawn when the terminal is narrower than 60 columns, or, before a filter is set, when
fewer than 8 cells are left for it after the counts and the summary (with a filter set, only when
the summary itself leaves no room); the line then opens in the status bar.
Plain words search subjects, the names and addresses of senders and recipients (To, Cc
and Bcc) and stored bodies; `from:`, `to:` (also Cc and Bcc), `subject:`, `has:attachment`,
`is:unread`, `is:starred`, `before:YYYY-MM-DD`, `after:YYYY-MM-DD` (local dates, by when the mail
arrived) and `account:<name>` narrow it; quote values with spaces (`from:"Dana Reyes"`). A thread
matches when one of its messages matches all of them. Anything else is searched as words, and the
header's search slot shows how Rail read your filter (`/ is:unread · words: form:dana`), so a typo
such as `form:dana` shows up as a word. `Enter` keeps the filter; `Esc` clears it. The filter
searches the local store only (`rail search` also asks each account's server).

`S` saves the filter in force under a name, so you never type it again: the prompt asks for a
name of 1 to 40 letters, digits, `-` or `_`, `Enter` saves it and `Esc` cancels. A filter that
narrows nothing — only characters the filter ignores, such as `* ( )` — is not saved; Rail says
`That filter narrows nothing; add a condition to save it.` A saved search
then behaves like a mailbox — its own row in the sidebar under `SEARCHES`, an unread count, a
`Search: <name>` palette entry, a header title, paging and `Esc` — and lists every folder of
every account narrowed by that filter, so it finds archived mail an inbox view would not. `/`
narrows a saved search further, and `Esc` on the reopened filter line clears only that live
filter; it never removes a saved search. `S` inside an open saved search saves only what you
typed after `/`, not that text joined to the open search's query: to save the narrower search,
type the whole filter. An `account:<name>` inside a saved query still narrows
to that account. A saved `before:`/`after:` keeps the date you typed, so a saved date ages: a
search for `after:2026-01-01` means that one day for ever, not "since a year ago". `r` and `d`
always act on the search the prompt or the question named: if the list changed while one of them
was open — an earlier save or removal was answered underneath it — Rail says
`The saved searches changed; try again.` and changes nothing. Rail keeps at most 20 saved
searches, in `[[saved_searches]]` in `config.toml`, where they are yours to read, edit and share.
On a large mailbox each counted search costs a query of its own on every sidebar refresh, so
`:search-counts` turns the counts off.

`:` opens the command palette: type a few letters of a key action for the current view, an account,
a label or a merged mailbox (`Mailbox: Starred`, `Mailbox: Drafts`, `Mailbox: Sent`,
`Mailbox: Trash`: that mailbox of every account together, newest first) and press `Enter`; the
key bound to each command is shown beside it. A label is listed as
`Label: AXON · Inbox/Infrastructure/Linode`: the account's label and the folder's whole path,
folded away in the sidebar or not. Each saved search is listed as `Search: needs-reply`, and
beside it a `Rename search: needs-reply` and a `Remove search: needs-reply` entry, which open
the same prompt and the same question as `r` and `d` on its sidebar row.
`:errors` lists the session's recent errors; every sync pass
that failed for good (one a retry will not mend) is listed there once, after its account's name,
while `:errors` still holds it. While one of them is an action Gmail refused, `j`/`k` or `↓`/`↑`
highlight an entry and `Enter` sends that action again. `:hints` turns the key hints off
(`hints: off`), or back on (the hints are the confirmation: no notice), and saves the choice
(`hints = false` under `[general]`; turning them on removes the key); a save that fails says
`Could not save the hints setting: …` and keeps the choice for this session. `:clock` hides the
header's clock (`clock: off`), or shows it again (`clock: on`), and saves the choice
(`clock = false` under `[general]`; showing it removes the key); a save that fails says
`Could not save the clock setting: …` and keeps the choice for this session. `:time` switches every
time of day on screen (the list, `● Synced 14:47`, the header's clock, the Drafts list, `:errors`,
the reader's `04:32` and its `Date: Fri 10 Oct 2025 12:40` lines) between 24-hour and 12-hour
(`2:47 pm`, `4:32 am`, `Date: Fri 10 Oct 2025 12:40 pm`) and saves the choice (`time = "12h"` under
`[general]`; 24-hour removes the key); a save that fails says `Could not save the time setting: …`
and keeps the choice for this session. `:thread-order` turns the reader's older messages round
(newest first, or oldest first again), in the open thread too, says `thread order: newest` or
`thread order: oldest`, and saves the choice (`thread_order = "newest"` under `[general]`; oldest
first removes the key); a save that fails says `Could not save the thread order: …` and keeps the
choice for this session.

`:dim-read` cycles how read threads look in the list: `dot` (the default) keeps today's small
muted dot, `subject` also dims the subject, and `all` dims the sender, subject, count and time.
Dimming uses the current theme's muted colour. Unread threads stay bold; the cursor highlight,
account badges, selection marks and action errors keep their colours. The command says
`dim read: dot`, `dim read: subject` or `dim read: all` and saves `dim_read` under `[general]`;
returning to `dot` removes the key. A save that fails says
`Could not save the read dimming setting: …` and keeps the choice for this session.

`save-search` names the filter in force and saves it, exactly as `S` does; it is offered only
while a filter is set. `rename-search <name>` and `remove-search <name>` rename or remove one
saved search — one entry each, per search, so the palette says which — and are offered only once
there is one. `search-counts` hides the unread count beside each saved search
(`saved-search counts: off`), or shows it again (`saved-search counts: on`), and saves the choice
(`saved_search_counts = false` under `[general]`; showing them removes the key); a save that
fails says `Could not save the saved-search counts setting: …` and keeps the choice for this
session.

`rich-html` shows HTML mail as plain text (`HTML mail: plain text`), or with its styles again
(`HTML mail: with its styles`), in the open reader and every one opened after, and saves the
choice (`rich_html = false` under `[general]`; styles on removes the key); a save that fails says
`Could not save the HTML mail setting: …` and keeps the choice for this session.

`images` hides the pictures in HTML mail (`images: off`), or shows them again (`images: on`), in the
open reader and every one opened after, and saves the choice (`images = false` under `[general]`;
showing them removes the key); a save that fails says `Could not save the images setting: …` and
keeps the choice for this session.

`remote-images` loads remote pictures in every thread from now on (`remote images: on`), the open
one too, or holds them back again until `i` (`remote images: off`), and saves the choice
(`remote_images = true` under `[general]`; off removes the key); a save that fails says
`Could not save the remote images setting: …` and keeps the choice for this session.

`?` shows the keys of the view you are in, marking with `*` the ones set in
`config.toml`; `?` again (or whatever `global.help` is bound to), `Esc` or `q` closes it. Help and
`:errors` scroll with `j`/`k` or `↓`/`↑`; a `↓ more` or `↑ more` line (`v more` and `^ more` with
`icons = "ascii"`) says there is more beyond the box.

The status bar shows, on the right, each account's sync state (`✓` caught up, `⟳` syncing, with
a count during the first sync, `retrying`, `stopped`, and `!` for `needs login` or
`sync failed`). After the state come `sending…` while a message is on its way,
`saving to Gmail…` (`saving to Microsoft 365…`) while a draft is saved, `2 queued` for actions on
their way to the account's server and
`1 failed` for actions it refused; see
[Doing things to mail](#doing-things-to-mail).
With threads selected, the header's left part ends with `3 selected`. Whether the daemon can be
reached shows in the header's summary (`connecting to the daemon…`,
`daemon unreachable (Ctrl-r retries)`, or `daemon reachable again (Ctrl-r resumes live updates)`
once a message fetch gets through after the connection was lost); notices show on the status bar,
in place of the key hints. When the daemon cannot be reached the list still reads from the store;
`Ctrl-r` reconnects (and starts a sync) and resumes live updates. A store read that fails says so
where it was shown (the list, the top of the sidebar, or the reader's subject line) and in the status
bar; `:errors` keeps it.

### Themes

Rail paints itself from a colour scheme of its own, on any terminal that shows 24-bit colour
(`COLORTERM` is `truecolor` or `24bit`). `:theme` lists the 769 schemes of the
[iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes) collection (MIT),
`Rail default` first; letters filter, `↓`/`↑` and `Ctrl-n`/`Ctrl-p` move, and the screen takes
each scheme as the highlight reaches it. `Enter` keeps it and writes `theme = "Name"` under
`[general]` in `config.toml` (choosing `Rail default` removes the key); `Esc` puts the previous
look back. `:accent` does the same for the accent (the focused pane's border, box borders, titles
and headings, what you type in the palette and the filter, the sync spinner): `scheme default`
or one of the scheme's sixteen palette slots, saved as `accent = N`. The other panes' borders take
the muted colour
(palette slot 8 of a scheme) whatever the accent. The status bar confirms `theme: Dracula` or
`accent: 5 magenta`; a save that fails says `Could not save the theme: …` (`:errors` keeps it)
and keeps the look for this session.

Without truecolor Rail keeps the terminal's own colours and the status bar says
`theme needs a truecolor terminal` when a theme is configured and when `:theme` opens. `:theme`
then does not preview (its hint row says `no preview without truecolor`) and `Enter` still saves
the choice; `:accent` still previews a palette slot, which uses the terminal's own colours there.
Inside tmux, `COLORTERM` must reach the inner shell (see Troubleshooting). An account's own
`color` in `config.toml` is kept whatever the theme.

## Doing things to mail

From the thread list or the reader: `e` archives, `d` moves to the account's trash (Rail asks
first, `Trash "subject"?` or `Trash 3 threads?`, and says what the trash does: Gmail keeps trashed
mail in its Trash for 30 days, and on Microsoft 365 it `Moves to Deleted Items.`; Rail never
deletes mail for good), `s` stars or unstars (it stars unless every thread is already
starred), `u` marks read or unread (it marks read if any thread is unread). An action applies to
the whole thread, as in Gmail. Over 50 threads (`bulk_confirm` under `[general]`) `e`, `s` and `u`
ask first too, naming the count, for example `Archive 1,234 threads?`; `d` always asks. The list
changes at once; the daemon sends the change to the account's server (Gmail or
Microsoft 365) in the background, so it also goes through if you quit Rail.

`l` opens the account's labels: type to filter, `Space` ticks or unticks, `Enter` applies (labels
you ticked are added, labels you unticked are removed; for a selection, the ticks start from every
label any selected thread carries), `Esc` cancels. `Down`/`Up` (or `Ctrl-n`/`Ctrl-p`) move,
because letters filter. On Gmail, `m` opens the same list to move: `Enter` adds the highlighted
label and archives the thread. Gmail's system folders (Inbox, Sent, Trash and the like) are not in
the list, and Rail does not create labels. A selection that spans two accounts cannot be labelled
or moved at once; Rail says `Labels belong to one account: select threads from one account.`

`Space` selects the thread under the cursor (a `▸` replaces its dot, `>` with `icons = "ascii"`)
and moves down; the header counts the selection (`3 selected`). With a selection, an action
applies to every selected thread, then the selection clears; `Esc` clears it. The selection stays
while you change the filter or the mailbox. In the reader, the keys act on the open thread, not on
the selection, which stays as it was (`Space` still pages down, and selecting is for the list);
archive, trash and move close the reader.

`* a` selects every thread this mailbox and filter match — newest first, at most 10,000, and the
header then reads `10,000 selected (more match)`; when nothing matches it says `Nothing matches.`
and selects nothing. `* f` puts the sender of the thread under the cursor into the filter line as
`from:<address>` and selects every thread with any message from that address, as the same `from:`
filter would show them: in the filter line that form matches that address exactly, where a plain
`from:dana` matches every thread whose sender's name *or* address contains `dana`. The sender is
that of the thread's newest message from someone else: your own messages (from the account's
address, or in its Sent or Drafts) are passed over, so a thread you answered last selects by the
person you answered. A thread with no such message with a readable sender, or one whose address
the filter cannot carry, says `This thread has no sender to select by.` and changes nothing. If
you change the mailbox or the filter before `* a` or `* f` has answered, its answer is dropped and
nothing is selected.

`v` starts a range at the thread under the cursor: the bar says `range from here`, and while the
range is open its first key hint is `Esc range`. Move as usual, then `v` or `Esc` ends the range
and keeps every thread it covered; `Esc` again clears the selection, as before. If the thread you
started from has left the list by then, Rail says `The range start is no longer in the list.` and
selects nothing.

`J` or `Shift-↓` extends the selection down from the thread under the cursor, and `K` or `Shift-↑`
up: the first press selects that thread and the next, and each further press one more. Going
back toward where you started unselects what the extension added; threads you had selected before
stay selected. Any other key ends the extension and keeps what it selected, so `J` after a move
starts again from the new place. A terminal that does not report Shift with the arrows still has
`J` and `K`; elsewhere in Rail, the Drafts folder included, a Shift-arrow moves like the plain
arrow, and there `J` and `K` move too. While `v` has a range open, `J`, `K` and the Shift-arrows
only move. If a sync reads the list again while you extend and the thread you started from leaves
it, the extension carries on from where the cursor was. A selection holds at most 10,000 threads:
an extension or a `v` range it stops says `The selection holds at most 10,000 threads.`

A bulk action goes out as one request per 200 threads per account, and one `z` takes the whole set
back, reporting `undone` once; an undo that could take only part of it back says `undone in part:
some were already sent`. When one of the requests fails, its threads come back to the selection and
its error stays on the status bar while the others answer.

After an action the status bar says what happened, in place of the key hints, for example `archived 3 · z undoes`. For ten
seconds `z` takes it back; afterwards it says `Too late to undo.`, and with nothing done,
`Nothing to undo.` If the daemon has not answered yet, `z` says `undoing…` and finishes when it
does. Only the most recent action can be undone, and marking a thread read by opening it has no
undo. `z` works anywhere in the mail screen. An earlier action whose answer comes after a newer one
still shows its error if it failed (its selected threads come back to the selection) and is still
undone if `z` was pressed for it; that undo says only `too late to undo` when it came too late.

Queued moves, label changes and undo keep targeting the original folder in the original account
across syncs. If that folder has been deleted, Rail does not substitute another folder.

Opening an unread thread marks it read at once in the reader; the list row and the unread counts
follow a moment later. `u` marks it unread again.

Each account in the status bar shows actions on their way to its server (`2 queued`) and actions
the server refused (`1 failed`). The daemon sends the actions on one thread in the order you made them;
an action waiting to be tried again holds back only the later actions on its own threads. When
Gmail cannot be reached it tries again after 1, then 5, then 15 minutes, then every 15 minutes,
and gives up on an action after 20 attempts. While an account needs a login, its actions wait and
are sent after the login, and an action still waiting when Rail starts again is sent. If recording
a server result in the local store fails, that account's queue waits while the daemon retries the
store write after one second, doubling the wait up to one minute. It keeps the server result and
does not repeat the provider action; the queue continues once the write succeeds. When Gmail
refuses an action for good (the thread or the label no longer exists), Rail undoes it in the
list, shows "Could not archive: …" on the thread's row, and keeps the error in `:errors`, where
`Enter` on it sends the action again. Refusals are listed in `:errors` again whenever Rail
connects to the daemon, so one that happened while Rail was closed or disconnected is not lost.
A later sync that brings Gmail's state of that thread clears the refusal and its `1 failed` (a
snoozed thread's failed return excepted: see [Snooze](#snooze)).
On a Microsoft 365 account the same keys act through Outlook's folders, where a message lives in
one folder. `e` moves the thread's Inbox messages to Archive; on a mailbox with no Archive folder
it says `This mailbox has no Archive folder; press m to move.` (Rail never makes one). `d` moves
the thread's messages to Deleted Items (the prompt says `Moves to Deleted Items.`). `m` lists
Inbox, Archive, Junk Email and your own folders, and moves the thread's messages to the one you
pick. Archive, trash and move leave your own messages in Sent Items and Drafts where they are.
`s` flags the newest message, or clears every flag of a flagged thread; `u` marks every unread
message read, or the newest message unread. `l` answers
`Outlook uses folders, not labels: press m to move.` A selection may hold threads of both kinds:
each account gets its own change, and `l` is refused for the whole selection. `m` is refused
for a selection spanning accounts: `Labels belong to one account: select threads from one account.` When Outlook
refuses some messages of a thread (one deleted meanwhile, say), the others still change and the
refusals are logged; only when it refuses every message does the action fail. Opening an unread
thread marks it read in Outlook (the status bar shows `1 queued` meanwhile).

### Snooze

`h` snoozes the thread under the cursor, the selection or the open thread (which closes): Rail
archives it now and puts it back in the inbox at the time you choose, unread, first under TODAY.
The chooser offers `Later today` (18:00, before 17:00), `Tomorrow` (08:00), `This weekend`
(Saturday 08:00, Monday to Friday) and `Next week` (Monday 08:00), each with its time; a choice
that falls at the time of an earlier one is left out (on a Friday, this weekend is tomorrow).
There is no calendar to click through: type a time instead, in numbers or in words.

- A time of day: `18:00`, `6pm`, `6:30 pm`, `noon` (today, or tomorrow once it has passed).
- A span: `3 days`, `in 2 hours`, `an hour`, `90 min`, `1 week`, `1 month`, `+3h`.
- A day: `tomorrow`, `tonight`, `later today`, `this weekend`, `next week`, `fri`, `next monday`.
- A date: `nov 3`, `3 nov 2026`, `november 3rd`, `2026-11-03`; without a year, a date that has
  passed this year means next year's.
- A day with a time, in either order: `tomorrow 9am`, `9am fri`, `nov 3 at 14:00`.

24-hour and am/pm times both work whatever `time` is set to. A day without a time comes back at
08:00, `tonight` at 20:00 and `later today` at 18:00. What you type is offered first as `At` with
its time; typing a choice's name (`tomorrow`) picks that choice. Text Rail cannot read says `Try
6pm, tomorrow 9am, 2 days, nov 3 or fri.`, a time that has passed `That time has passed.`, one
less than about a minute ahead `That is too soon.`, and one more than a year ahead `That is more
than a year away.` `Enter` checks the time against the clock once more, so one that became too
soon while the chooser was open is refused there and nothing is sent. The last time you typed
is offered first next time as `Last used`, read again against the clock (`2 days` counts from
when you open the chooser), unless a choice already offers that time; Rail keeps it in the state directory
(`snooze-last`), so it survives a restart. `Enter` snoozes, `Esc` cancels, `Down`/`Up` move.

The status bar says `Snoozed until Mon 08:00 · z undoes`, and for ten seconds `z` takes the snooze
back, as with archive. On a thread that is already snoozed the chooser's title says until when;
choosing a time moves the snooze, and its first row, `Return to inbox now`, puts the thread back
at once (`returned 1 to the inbox`). While anything is snoozed, `Snoozed` is listed under UNIFIED in
the sidebar with the number of snoozed threads (`◷`, or `^` with `icons = "ascii"`); `:` and
`Mailbox: Snoozed` open it at any time, and an empty one says `Nothing is snoozed.` A thread whose
snoozed mail was all trashed, moved or put back in the inbox since has nothing left to bring back,
so it is neither listed nor offered `Return to inbox now`. The reader
shows `Snoozed until Mon 13 Oct 2025 08:00` under a snoozed thread's recipients. Drafts cannot be
snoozed (`Not available on drafts`).

A selection is snoozed as one. The chooser's title counts it (`Snooze 3 threads until`, with
`Return to inbox now` first when some of it is snoozed), and the snooze takes the threads that
have mail in the inbox, or are snoozed already, and leaves the others where they are; the status
bar counts both (`Snoozed 118 until Mon 08:00 · z undoes · 2 not in the inbox`). Only a snooze
with none to take is refused (`Only a thread in the inbox can be snoozed.`). `Return to inbox now`
counts what came back as its requests answer (`returned 250 to the inbox`), and a request that
fails says why and gives its threads back to the selection.

New mail in the thread brings it back early; your own reply, which lands in Sent, does not, and
neither does a message of the snooze put back in the inbox from another device (snoozing the
thread again then archives it with the rest). When threads come back the status bar says `Back
from snooze: <subject>` or `3 threads back from snooze`.

Neither Gmail nor Microsoft 365 offers a snooze Rail can use, so Rail keeps the time itself: other
devices show the thread archived until it comes back, and it comes back while the daemon runs (see
[The background daemon](#the-background-daemon)). The return puts back exactly the messages the
snooze took out of the inbox: on Gmail it adds `INBOX` to them (one you trashed or reported as
spam meanwhile stays there), and on Microsoft 365 it moves them from Archive back to Inbox (one
you moved to another folder meanwhile stays there). If the server refuses the archive, the snooze fails with it (`Could not archive: …`, and
`Enter` in `:errors` sends it again); snoozing the thread again starts afresh. A return the server
cannot take right away is tried again like any action; one it refuses, or that gives up after 20
attempts, leaves the thread archived and stays in `:errors` (`Could not return to the inbox: …`)
through later syncs, where `Enter` sends it again; a retry leaves mail you trashed or reported as
spam since where it is. It leaves `:errors` once it is sent, once the thread is back in the inbox
by other means, once none of its messages is still archived (all trashed, reported or filed
elsewhere), or when you archive, trash or move the thread in Rail. So a return the server keeps
refusing stays until you send it again with `Enter` in `:errors` and it goes through, or you
archive, trash or move the thread in Rail to give it up. Removing an account
leaves its snoozed threads archived, since nothing syncs that account any more.

## Writing mail

From the thread list or the reader: `c` writes a new message, `r` replies to the latest message of
the thread, `R` replies to everyone and `f` forwards it. In the list they act on the thread under
the cursor; in the reader, on the open thread. In the unified inbox with more than one account,
`c` first asks which account to write from; elsewhere it writes from the account you are looking
at. One message is written at a time (`Finish or close the message being written first.`). A
reply goes to the message's `Reply-To` when it has one, otherwise to its sender; `R` adds the
other recipients except you, keeping the old `Cc` in `Cc`. A reply starts with `Re:` once and the
quoted text under `On <date>, <name> wrote:`; a forward starts with `Fwd:`, no recipients, and the
original under a `Forwarded message` line, with its attachments. The links the quoted or
forwarded text marks follow it after a blank line, one per marker: `[1] https://…`, or
`[2] (link not included)` for an address Rail would not open. If the message's text is not in
the store yet, Rail fetches it first (`fetching the message…`). Writing from a Microsoft 365
account works the same way; [Microsoft 365 mail](#microsoft-365-mail) says what differs.

**A new message** starts with two prompts on the bottom row. `To:` completes from the addresses
your mail has taught Rail: type a few letters and matching addresses appear above, none
highlighted; `Tab` and `Shift-Tab` (or `↓` and `↑`) move through them. `Enter` takes the
highlighted one, or the text you typed when none is, and a comma takes the highlighted one and
starts the next recipient (with none highlighted it is typed as a comma). Then `Subject:`.
Each address is checked as you press `Enter` and the prompt stays open with the reason when one
is wrong. A recipient is a plain `user@domain` or `Name <user@domain>`; a quoted name with a comma
(`"Reyes, Dana" <dana@example.com>`) is one recipient. `Esc` in either prompt cancels the
message; nothing is saved. A reply or forward skips the prompts and opens the editor at once.

**The editor.** Rail runs `general.editor`, else `$VISUAL`, else `$EDITOR`, else `vi`, on a
file in its cache folder (`compose/<draft>.eml`, mode 0600). The command is split like a shell
command line (quotes work) but no shell runs, so pipes and `~` are not interpreted. An editor that
returns at once needs its wait flag: use `editor = "code --wait"`. While the editor runs Rail
steps aside and ignores `Ctrl-C` and `Ctrl-\`; it takes the terminal back when the editor exits.
The file holds four headers, a blank line and the body:

```
To: Dana Reyes <dana@example.com>
Cc:
Bcc:
Subject: Lunch

Are you free on Thursday?
```

`To`, `Cc` and `Bcc` take comma-separated recipients; a header is one line (a line starting with
a space continues the one before). Any other header, or a line that is not `Name: value`, is
refused: Rail names the line and opens the editor again on the same text. Rail also accepts a file
the editor saved with Windows line endings (CRLF) and keeps the message as plain lines. If the
editor exits with a non-zero status Rail asks `Keep what you wrote?`: yes saves the text, no keeps
the draft as it was. If the editor cannot start Rail says `cannot run …` and the review opens with the text it
had. The file is kept until a save succeeds, and also when the editor cannot run or its file
cannot be read back. If Rail stops before that (a crash, a closed terminal), the next editor round
for that draft opens on the file as it was left, headers included (so changes made with `t` after
reopening the draft are replaced by the file's headers), and says
`opened the text from your last editor session`. Sending or discarding the draft without opening
the editor uses the stored text, and the file stays until it is removed after seven days. Quitting
that editor with an error and answering no to `Keep what you wrote?` keeps the draft as it was. A
new message is saved before its first editor round, so it is recovered like any other draft. A
`new.eml` (left when that first save failed) is never reused for another message; it is removed
after seven days.

**The review.** When the editor returns, Rail shows the message as it will go: `From`, `To`, `Cc`
and `Bcc` (when used), `Subject`, then the attachments with their count and total size, then the
body, with long runs of quoted lines folded to `[n quoted lines]`. A recipient with an
international domain is followed by the domain's ASCII form, as `xn--` addresses are what
the mail system sees. The status bar names the keys:

| Key | What it does |
|---|---|
| `y` | Send. |
| `s` | Save the draft to the account's Drafts (`s save to Gmail` or `s save to Microsoft 365` in the status bar). |
| `e` | Edit again in your editor. |
| `t` | Change `To`, `Cc`, `Bcc` and `Subject`, one prompt after another. |
| `a` | Attach a file: type its path (`~` works, `Tab` completes names from the folder, relative paths are made absolute) and press `Enter`. |
| `x` | Remove the attachment on the cursor row: a file you attached or one a forward carried along. |
| `q` or `Esc` | Close and keep the draft: `draft kept`. |
| `d` | Discard the draft after `Discard this draft? (y/n)`: `draft discarded`. |
| `j` `k` `↑` `↓` `PageUp` `PageDown` `Space` | Move through the message. |

These are the `compose.*` actions in the key table and can be changed. While a `t` or `a` prompt
is open the status bar shows the prompt's keys instead: `Enter accept  Tab complete  Esc cancel`.
`y` is refused with a reason when there is no recipient (`Add a recipient first`), while a save
is pending, or after a save failed. If a save failed, `q` asks `Discard the unsaved changes?` and
then says `unsaved changes discarded`; a message closed before anything was saved says
`The message was not saved.`

**Sending.** `y` hands the message to the daemon, which sends it ten seconds later. The status
bar says `sent · z undoes` and `z` takes it back inside that window: the review opens again with
the message as it was. After the ten seconds the account shows `sending…` and the message goes
through Gmail like the actions in [Doing things to mail](#doing-things-to-mail). Closing the
review with `q` before the daemon has the message does not stop the send: its `sent · z undoes`
(or `Could not send`) still comes, even if another message is open by then. `z` then cancels the
send but cannot reopen its review; the draft stays in Drafts, to open from there. Until
the last message is queued, `y` and `s` on any message are refused (`Wait for the last message
to be queued.`), and so are `e`, `a` and `x` on the review being sent or saved to the account's
Drafts. A draft whose send or save (`s`) is on its way does not open from the Drafts folder or
`:drafts` (`This draft is being sent; wait for it to be queued.`, or
`This draft is being saved to Gmail; wait a moment.`, or
`This draft is being saved to Microsoft 365; wait a moment.` on a Microsoft 365 account); other
drafts do. If the connection to the daemon is lost first, Rail cannot tell whether the message
was queued and says `Could not confirm the send: … · check Sent and Drafts`
(`Could not confirm the save to Gmail: … · check Drafts` for `s`, or
`Could not confirm the save to Microsoft 365: … · check Drafts`). If it is in Sent, it went; if
it is still in Drafts, open it and press `y` again: that is safe, since the daemon refuses a draft
it already has queued (`this draft is already being sent`) and a sent draft is gone. A message is
never sent twice: before any later attempt (a retry, or a start after the daemon stopped
mid-send) the daemon asks Gmail whether a message with that `Message-ID` has the `SENT` label and,
if so, counts it as sent. An unsent copy in Gmail's Drafts does not count. A reply whose
conversation was deleted in Gmail meanwhile goes out on a conversation of its own. When Gmail
refuses a message for good, the review opens again with the error
(`Could not send: Gmail refused the request (HTTP 400)`) and the failure is listed in `:errors`.

**Drafts.** Every editor round, header change and attachment change saves the message in the
local store, so a crash or a closed terminal loses nothing. A reply whose original message has
since left the store keeps its threading. A draft stays on this computer until you press `s` on
its review: Rail saves any text not stored yet, saves the draft to Gmail's Drafts ten seconds
later (Outlook's Drafts for a Microsoft 365 account), closes the review and says
`saved to Gmail · z undoes` (`saved to Microsoft 365 · z undoes`). `z` inside that window cancels
it (`kept on this computer`). The review closes after `s`, so pressing `s` again means opening the
draft again from the Drafts folder or `:drafts`; done inside the ten seconds, it makes one save
with the newest text, and `z` after it cancels that save. `s` is refused when the message holds
nothing at all. The status bar shows `saving to Gmail…` (`saving to Microsoft 365…`) while a save
is on its way. A save the provider refuses on the spot keeps the review open
(`Could not save to Gmail: …`, or `Could not save to Microsoft 365: …`); one that fails later is
listed in `:errors` (`Could not save the draft to Gmail: …`, or
`Could not save the draft to Microsoft 365: …`), where `Enter` retries it. The first save
creates the draft in Gmail and the next ones update it, so the draft keeps one place in Gmail's
Drafts and opens on your phone. Rail's version wins: if the same draft was also edited in Gmail,
the next `s` overwrites that edit. Sending a draft that was saved to Gmail deletes its Gmail copy
once the message is sent; if that delete fails it is listed in `:errors`, where `Enter` retries it.

Creating the Gmail draft is tried once and never repeated inside one attempt, as a send is. When
that attempt may have reached Gmail but its answer was lost, the retry later can leave a second
copy of the draft in Gmail's Drafts. Delete the extra one there; updating a draft, deleting it
and listing drafts are retried freely and cannot do this.

**The Drafts folder.** Each account's `Drafts` in the sidebar lists its drafts in one pane
(`Drafts` under UNIFIED lists every account's), newest first, up to 500: the drafts saved on this
computer and the Gmail drafts written elsewhere (on your phone, in the browser). A row shows the
date, the recipients and the subject, like a thread; a draft kept only on this computer says
`(local)` where the recipients go and has `✎` (`*` with ASCII icons) before its subject, and a
draft saved to Gmail is listed once. `Enter` opens a draft at its review. A Gmail draft is
fetched first (`fetching the draft…`), together with its
attachments, and from then on is a draft of this computer too, its Gmail copy updated by `s`.
A Gmail draft Rail already holds that was edited elsewhere since is listed again as a Gmail row;
opening it opens Rail's copy, as Rail's version wins. `d` asks `Discard this draft? (y/n)`; yes
deletes it, and its copy in the account's Drafts with it (Outlook moves it to Deleted Items).
While a save to Gmail is still on its way the discard is refused
(`draft: saving to Gmail, try again in a moment`, or
`draft: saving to Microsoft 365, try again in a moment`): try again once it has landed. A Gmail
draft that was deleted elsewhere before you opened or discarded it says
`Gmail no longer has this draft`; one that cannot be fetched says `Could not open the draft: …` or
`Could not discard the draft: …` with the reason. Archive, star, read, labels, move,
select, selecting all or by sender, the range, reply, forward and the filter
(`e`, `s`, `u`, `l`, `m`, `space`, `* a`, `* f`, `v`, `r`, `R`, `f`, `/`) say
`Not available on drafts`; `Esc` does nothing there and `c` writes a new message as everywhere.
The search slot is empty in this folder: the pane is not filtered. The pane is read again whenever
sync or the outbox
changes something. `:drafts` (`Open a draft` in the palette) lists the same rows for the account
in view; in the unified inbox and the merged mailboxes it merges every account's, and `Enter`
opens one the same way.
A Microsoft 365 account's rows are its Outlook drafts and behave the same way: `Enter` imports
one with its attached files, `d` says `It is deleted from Outlook's Drafts.` and deletes it there,
and one deleted in Outlook first says `Microsoft 365 no longer has this draft`.

**Files of an imported draft.** The attachments of a Gmail draft are fetched once, when you open
it, into private files the daemon keeps in the `draft-files` folder of the data directory
(folders 0700, files 0600). Later saves and the send use those files, not Gmail's copy. Discarding
the draft removes its files, and so does a completed send; when the daemon starts it removes every
file no draft refers to, which catches any a removal could not.

**Limits.** A message has at most 100 recipients, 50 attachments and 25 MiB in all (Gmail's
limit, kept for Microsoft 365), 10 MiB of text and a one-line subject. Attachments count as they are
sent, a third larger
than the files (so about 18 MiB of files fit); the review's total shows the files' own sizes, and
a file that would take the message over is refused when you attach it, naming the size the
message would have. File names are cleaned before they are sent. Attachments come from files you
name by path; a forward carries the original's attachments along, and they count in the total. Only
regular files can be attached, and
they are read when the message goes out: one that was moved or deleted in the meantime fails the
send (`Could not send: could not read an attachment of this message`) and the review opens again.

**Your address and signature.** `From:` is the account's address, with `from_name` before it when
set. A `signature` file is added under `-- ` to every new message, reply and forward. Both are
described under [`[[accounts]]`](#accounts).

## Account setup

Rail ships no OAuth client of its own: you register one per provider once (or use a preset from
your organisation), then log in. The wizard walks you through the same steps shown here.

### Gmail / Google Workspace

1. In Google Cloud Console create a project and enable the **Gmail API**.
2. **OAuth consent screen**: External. **Publishing status must be "In production"**, even
   unverified. An app left in "Testing" has its refresh tokens expire after 7 days, and Rail will
   keep asking you to log in.
3. **Credentials**: create an **OAuth client ID** of type **Desktop app** and copy the client id
   and secret. A desktop-app secret is not actually secret; keeping it in `config.toml` is fine (or
   keep it in [1Password](#tokens-in-1password)).
4. Log in with `rail setup`, or:

   ```sh
   rail account add gmail --address you@example.com \
     --client-id XXXX.apps.googleusercontent.com --client-secret GOCSPX-...
   ```

   A browser opens for consent. Google warns that your own app is unverified; continue.

<!-- guide:google v1 -->
**The same steps with `gcloud`.** `gcloud` can create the project and enable the API, but it cannot
create a desktop OAuth client, so steps 2 and 3 stay in the browser. Pick a project id: 6 to 30
characters, lowercase letters, digits and hyphens, starting with a letter and not ending with a
hyphen:

```sh
gcloud auth login
gcloud projects create <project-id>
gcloud config set project <project-id>
gcloud services enable gmail.googleapis.com
```

Then, in the browser:

- the consent screen at `https://console.cloud.google.com/apis/credentials/consent?project=<project-id>`:
  External, then publish ("In production");
- the credentials page at `https://console.cloud.google.com/apis/credentials?project=<project-id>`:
  Create credentials, OAuth client ID, Desktop app, and download the JSON file.

An AI agent connected over MCP can walk you through exactly these steps (see
[Using Rail from an AI agent](#using-rail-from-an-ai-agent-mcp)).

### Microsoft 365 / Outlook

1. In Azure Portal, **App registrations** > New. Supported account types: pick what matches your
   mailbox (work/school, personal, or both). Redirect URI: platform **Mobile and desktop
   applications**, value `http://127.0.0.1`. Rail listens on `http://127.0.0.1:<random port>`, and
   Azure matches any port for a loopback redirect. If the portal refuses an `http://127.0.0.1` URI,
   add it by editing the app's **Manifest** (`replyUrlsWithType`) instead.
2. Under **Authentication**, set "Allow public client flows" to **Yes**. Rail is a desktop app and
   holds no client secret; without this Microsoft answers `AADSTS7000218`.
3. **API permissions**: Microsoft Graph delegated `Mail.ReadWrite`, `Mail.Send`, `User.Read`,
   `offline_access`.
4. Log in with `rail setup`, or:

   ```sh
   rail account add m365 --address you@example.com --client-id <application-id>
   ```

   The client ID is the **Application (client) ID** on the registration's **Overview** page, not
   the *Object ID*. For a single-tenant app ("Accounts in this organizational directory only") also
   pass `--tenant <tenant-id-or-domain>` (or fill in the wizard's *Tenant* field): Microsoft refuses
   the default `common` endpoint for single-tenant apps. Add `--tenant` too if your organisation
   restricts `common`.

<!-- guide:microsoft v1 -->
**The same steps with `az`.** The Azure CLI can do the whole registration of an app for work or
school accounts (single or multiple organisations); personal Microsoft accounts use the portal steps
above. Use the tenant id or a domain the tenant owns for `<tenant>`, and `AzureADMyOrg` (one
organisation) or `AzureADMultipleOrgs` for `<audience>`:

```sh
az version
az login --tenant <tenant>
az account show --query tenantId --output tsv
az ad app create --display-name Rail --sign-in-audience <audience> --public-client-redirect-uris http://127.0.0.1 --is-fallback-public-client true --query appId --output tsv
az ad sp show --id 00000003-0000-0000-c000-000000000000 --query "oauth2PermissionScopes[?value=='Mail.ReadWrite' || value=='Mail.Send' || value=='User.Read' || value=='offline_access'].{value:value, id:id}" --output table
az ad app permission add --id <app-id> --api 00000003-0000-0000-c000-000000000000 --api-permissions <mail-readwrite-id>=Scope <mail-send-id>=Scope <user-read-id>=Scope <offline-access-id>=Scope
```

`az ad app create` prints the Application (client) ID; use it as `<app-id>` and as `--client-id`.
`az ad sp show` lists the ids of the four Microsoft Graph permissions for `az ad app permission add`.
An administrator can then approve Rail for the whole organisation. Approval needs the app's identity
in the tenant (an "already exists" error from the first command means it is done); if the second
command reports that the permissions or the service principal are not found, wait a minute and run
it again, because directory changes take a moment to propagate:

```sh
az ad sp create --id <app-id>
az ad app permission admin-consent --id <app-id>
```

### Work or school accounts and admin consent

**Single or multi-tenant.** "Accounts in this organizational directory only" (single tenant) is
right for one organisation; log in through its tenant ID or domain. Choose "Accounts in any
organizational directory" only when several organisations share one registration; then `common`
works, but each organisation still has to consent.

**Finding the tenant ID.** In Azure Portal open **Microsoft Entra ID** > **Overview** and copy the
*Tenant ID*, or copy the *Directory (tenant) ID* from your app registration's **Overview**. A domain
the tenant owns (`contoso.onmicrosoft.com`, or a verified domain such as `contoso.com`) works in the
same places.

**Admin consent.** Many organisations stop users from approving apps themselves. An administrator
approves Rail once for everyone, in any of three ways:

- *From the wizard.* On the Details step tick **I am an administrator of this tenant**. Rail opens
  Microsoft's admin-consent page for your tenant, waits for the redirect to `http://127.0.0.1`, and
  confirms, then continues with your own login. The tenant must be a concrete tenant ID or domain,
  not `common`; the wizard asks for it if needed.
- *From the portal.* Open the app's **API permissions** and choose **Grant admin consent for
  <organisation>**.
- *During a normal login.* An administrator who logs in is shown Microsoft's consent page with a
  **Consent on behalf of your organization** checkbox; ticking it grants consent for everyone.

If you are not an administrator and your login fails with `AADSTS65001` or `AADSTS90094`, the
wizard shows the exact admin-consent URL. Press `c` to copy it, send it to an administrator, and
press `r` to retry once they have approved. Tenant policies (Conditional Access, app assignment,
MFA) can still intervene after consent; see [Microsoft sign-in errors](#microsoft-sign-in-errors).

### Microsoft 365 mail

What a Microsoft 365 account does today. It syncs, reads, acts on threads and writes mail over
Microsoft Graph.

- **Folders.** Every mail folder is synced, nested ones as `Parent/Child` (a `/` in a folder's
  own name shows as `∕`). Inbox, Sent Items, Drafts, Archive, Deleted Items and Junk Email are the
  account's system folders in the sidebar, under those names, and fill `Sent` and `Trash` under
  UNIFIED; every other folder is under `More folders`, nested ones as a tree. Search folders and
  Conversation History, RSS Feeds, RSS Subscriptions and Sync Issues are left out. Those last four
  are matched by their English names, so a mailbox in another language may show them.
- **Threads and stars.** A thread is an Outlook conversation, and a star is Outlook's flag.
- **The first sync** shows `⟳` and a running count with no total, because Microsoft gives no cheap
  one. It reads folder by folder, the inbox first. New mail then arrives within one
  `poll_interval`.
- **History.** With `history_days` set, Microsoft returns at most 5,000 messages per folder;
  mail in a folder beyond that is not synced.
  Raising `history_days` reads every folder again. When Microsoft forgets a folder's place (after
  a long time offline), or a folder is deleted, the account reads every folder again and removes
  what is gone, except while a folder's subfolders cannot be listed or a folder is given up on:
  then nothing is removed until a later full read (see `a folder cannot be synced` in
  [Troubleshooting](#troubleshooting)).
- **Search.** `from:`, `subject:`, `to:`, `body:` and plain words work; Gmail's `label:`, `is:`
  and `has:` do not. A search returns at most 1,000 results from the server.
- **Attachments.** A OneDrive or SharePoint link attachment is listed, and when Rail is asked for
  its bytes it says `This attachment is a link (OneDrive or SharePoint); open the message in
  Outlook on the web to reach it.` A message whose attachments are all inline lists none. When
  Microsoft refuses a message's attachment list, Rail keeps the list it already had rather than
  losing it, and does not ask again. For mail stored before 0.4.0 that kept list can be out of
  date: it may show an inline picture or an attached message as an ordinary file, so forwarding
  that message, or attaching it as the original, may try to send that part as a file. Microsoft
  rarely refuses the same list for good, and opening a picture that is missing fetches the
  message's body again, which corrects the list whenever Microsoft gives it. If a forward of such a message goes wrong,
  forward it from Outlook on the web.
- **Marking read.** Opening an unread thread marks its unread messages read in Outlook; `u`
  marks them read, or the newest message unread.
- **Acting on threads.** `e` archives to Outlook's Archive folder (a mailbox with no Archive
  folder says `This mailbox has no Archive folder; press m to move.`), `d` moves to Deleted
  Items, `m` moves to Inbox, Archive, Junk Email or one of your folders, and `s` flags the
  newest message (or clears every flag), each with `z` to undo. Your own messages in Sent Items
  and Drafts stay where they are. See [Doing things to mail](#doing-things-to-mail).
- **Folders, not labels.** `l` answers `Outlook uses folders, not labels: press m to move.`
  Outlook categories are not used.
- **Writing.** `c`, `r`, `R` and `f` write as on Gmail, and `y` sends with the ten second `z`
  undo. Rail writes each message as an Outlook draft: a reply, reply to everyone or forward is
  made on the original message, so it joins the Outlook conversation and others see it
  threaded; Rail then sets its subject, text, recipients and `Message-ID`. The text goes as plain
  text. `y` sends that draft, and the sent copy arrives in Sent Items with the next sync. An
  original still in Deleted Items keeps the threading; a reply or forward whose original is gone
  from Outlook for good (Deleted Items emptied) goes out as a new message, outside the
  conversation.
- **Drafts.** `s` saves the draft to Outlook's Drafts (`saved to Microsoft 365 · z undoes`); later
  saves update the same Outlook draft, and one deleted in Outlook is made again in Drafts (the
  deleted copy stays in Deleted Items). Sending a saved draft sends that very draft, so none is
  left in Drafts. Opening a draft written in Outlook fetches its current body and attachment
  listing, even when Rail has cached them; a failed fetch stops the import. Once imported, the
  local draft keeps your edits when reopened. `d` on it deletes it in Outlook (it goes to
  Deleted Items). An extra copy can stay in Outlook's Drafts:
  when Outlook made a reply, reply to everyone or forward draft but its answer was lost on the
  way back (a timeout, say) or could not be read, Rail makes another on the retry; and when a
  save made the Outlook draft but then failed for good (a permanent attachment error, say),
  discarding the draft in Rail leaves that copy. Delete the extra one in Outlook's Drafts.
- **Attachments.** At most 50 files and 25 MiB in all, as on Gmail (`the message is … MiB, larger
  than Microsoft 365's 25 MiB limit`). A file of 3 MiB or more goes up in 3.75 MiB pieces, and a
  failed upload is tried again later. A forward keeps the original's attachments in Outlook without
  downloading them; remove one in the review and it goes from the draft too. Rail matches its
  attachments to the draft's by name and the size Outlook lists for them, so a save or send uploads
  only a file that is new or has changed since (other bytes, or the same length written again). Rail
  writes the message as plain text, so pictures inside the body of a reply, a forward or an imported
  Outlook draft are dropped from it. Attached messages and OneDrive or SharePoint links stay as
  Outlook has them: Rail does not list them for removal and never downloads or uploads them.
- **Never sent twice.** Before any later attempt (a retry, or a start after the daemon stopped
  mid-send), Rail asks Outlook for a sent message with the draft's `Message-ID`, and about the
  draft it saved, and counts a sent one as sent.

### Presets for your organisation

An administrator can save every colleague the portal work: register the app once (as above, with
public client flows enabled and admin consent granted), then hand out a preset file that fills in
the wizard. A preset never contains tokens, and Rail never writes preset files.

```toml
name = "Contoso Microsoft 365"       # shown in the wizard's preset list
kind = "m365"                        # or "gmail"
client_id = "00000000-0000-0000-0000-000000000000"
tenant = "contoso.onmicrosoft.com"   # Microsoft only
# client_secret = "GOCSPX-..."       # optional; Gmail desktop clients need it
# help_url = "https://intranet.example.com/rail"   # optional "Need help?" link (F1 in the wizard)
```

`kind` and `client_id` are required; the rest is optional. Unknown keys and wrong types are
rejected with the file's path and line number. Users still enter their own address.

Publish a preset in either of two ways:

- as a file in `/etc/rail/presets/*.toml` on each machine, or a user's `~/.config/rail/presets/*.toml`
  (`$RAIL_CONFIG_DIR/presets` when that is set); the wizard lists these;
- on the intranet over `https`, so people can type the URL on the preset step. Rail refuses plain
  `http://`, redirects (an intranet URL behind single sign-on fails with "HTTP 302"; use the file
  instead) and bodies over 64 KiB, and never saves the fetched file.

A colleague then runs `rail setup`, chooses **Use a preset from my organisation**, picks the preset
(or types its path or URL), enters their address and logs in.

## Using Rail from an AI agent (MCP)

Rail is an [MCP](https://modelcontextprotocol.io) server, so an AI coding agent (Claude Code,
Codex, Cursor and similar) can do what the wizard does, including creating the Google project or
the Microsoft app registration. Register it once:

```sh
claude mcp add rail -- rail mcp
```

Other clients: run `rail mcp` as a stdio server. Then ask the agent to run Rail's `setup` prompt.
It checks `rail_status`, asks which providers you want, and walks through `rail_provider_guide`
one step at a time: it runs the `gcloud` or `az` commands with your approval, hands you the
browser for the pages only you can complete (the Google consent screen, the client download, every
login), and finishes with `rail_account_add` and `rail_wait`.

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

**What the tools cannot do.** No tool returns mail content (subjects, senders, snippets or bodies)
and none sends mail. No MCP tool archives, trashes, labels, snoozes or otherwise changes mail. No secret passes through the agent: `rail_account_add` takes the absolute path
of the client-secret JSON you downloaded, an `op://` reference or a preset, never the secret itself,
and a `client_secret` sent by an agent is refused. Tokens stay in the daemon. Tools refuse unknown
arguments and relative file paths.

**Confirmation.** `rail_account_remove` and `rail_secrets_configure` ask you to confirm in your
agent client's own dialog (MCP elicitation). The text is written by Rail, not the agent, with every
name, address and vault quoted and shown in plain ASCII so look-alike characters cannot pass for
others. It is bound to a digest of the exact plan: if anything changes before you answer, nothing
runs and you are asked again. No tool input confirms on your behalf. Declining, dismissing or a
client that cannot show the dialog changes nothing; the agent is told to ask you to run the
equivalent terminal command (`rail account remove <name>`, `rail secrets set`) instead.

**The honest limit.** This protects the MCP path only. An agent that can run shell commands runs
them as you: it could run `rail search`, read `rail.db`, drive `rail` in a pseudo-terminal or talk
to the daemon's socket, like any program you run. Rail cannot stop a program running as the same
user. There, the control is your agent client's approval of shell commands: keep it on, and read a
command that runs `rail` before you allow it.

## Security and privacy

**Tokens.** OAuth tokens are never written to `config.toml`. They live in one of:

- the **OS keyring** (default; service `rail`): Secret Service on Linux, the Keychain on macOS;
- a **private file** (`secrets = "file"`): `secrets.json` in the data directory, mode 0600, for
  machines without a Secret Service;
- **1Password** (`secrets = "1password"`), through the `op` CLI; see
  [Tokens in 1Password](#tokens-in-1password).

Tokens, client secrets and authorization codes never appear in logs, error messages or terminal
output. Files that hold mail or credentials are created owner-only (0600 files, 0700 directories)
and written atomically.

**What is stored locally.** For each account, `rail.db` holds every label; the envelope
(sender, recipients, subject, date, flags, labels, snippet) of every message inside
`history_days` (the whole mailbox by default); the text of bodies for threads with mail newer than
`body_cache_days`; and attachment names, types and sizes (never attachment contents, except those
of a Gmail draft you open, below). Drafts you write are kept there too, with the paths of the files you attached (the files are read when the message is sent,
so they must still be there then). A draft saved to Gmail also lives in Gmail's Drafts, which sync
brings in like other mail. Opening a Gmail draft copies its attachments into private files owned by
the daemon (the `draft-files` folder of the data directory, 0600 files in 0700 folders; see
[Writing mail](#writing-mail)); discarding the draft removes them, and the daemon removes any no
draft refers to when it starts. Saving an attachment (`s`, `S`, `w`) writes a private (0600) file
into `attachment_dir` — your download directory unless you set one — and never replaces a file
that is there: a clash becomes `2-name.pdf`, and a name beginning with a dot or a dash loses it,
so a save cannot hide a file in that directory or leave one shaped like a command-line option.
Opening an attachment writes a 0600 copy in a 0700 directory of its own under the cache
directory (`~/.cache/rail/attachments/` on Linux), removed a minute after you close the reader —
so a viewer that starts slowly still finds it — or at once when you quit; any copy older than 24
hours is swept when the daemon starts and before each open. On macOS, files
Rail writes are quarantined like downloads. The editor's working file lives in the `compose`
folder of the cache directory until a save succeeds, and one a crash left
behind is removed when
the mail screen next starts, once it is seven days old. HTML bodies
are converted to text by Rail's own scanner without running scripts or loading anything remote, so
syncing never fetches a tracking pixel. A body over 10 MiB is cut and says so. Mail that a search
finds on its server outside the window is kept for 14 days after it was last searched for. A snooze keeps,
in `rail.db`, when the thread comes back and the ids of the messages its archive took out of the
inbox; the last time typed into the snooze chooser is kept in `snooze-last` in the state
directory. Rail
changes mail on the server only when you act on it (archive, trash, star, read, labels, move),
send, or save or discard a draft. It never deletes mail permanently, except a Gmail draft you
discard or send (a Microsoft 365 draft you discard goes to Deleted Items), and the daemon is the
only process that talks to Gmail or Microsoft 365.

Remote content in mail is loaded only when you ask for it. `o` opens a message's HTML in your
browser, after you confirm once per session, and `g x` opens a link there, after you have seen its
address. The reader draws a message's remote pictures only after `i`, for a sender you trusted with
`I`, or with `remote_images = true`; the daemon fetches them as described in
[Reading mail](#reading-mail), and fetching one tells its sender you opened the mail.

**Mail and AI agents.** Through MCP, mail content never reaches an agent today. Text from outside
Rail (preset files, provider and system error messages) comes back to the agent marked as
untrusted: quoted on one line, cut to 200 characters, and preceded by a notice that it is data, not
instructions. The per-account [`ai_send`](#ai_send-what-an-ai-agent-may-do-with-your-mail) policy
will govern sending once send tools exist. See [the honest limit](#using-rail-from-an-ai-agent-mcp)
for agents with shell access.

**The daemon.** It listens on a Unix socket in a directory only you can open (0700); the socket is
0600, and connections from other users are dropped. On screen, text Rail did not write (a provider's
error, the system's) is shown on its own line, quoted, with control characters escaped.

**Logs.** `rail.log` never holds tokens or mail at the default levels. Rail keeps the HTTP and TLS
libraries (`hyper`, `hyper_util`, `h2`, `reqwest`, `rustls`) at `warn` whatever `RUST_LOG` says.
Setting a more specific target, such as `RUST_LOG=hyper::proto=debug` or
`RUST_LOG=rmcp::handler=warn`, overrides that and can write request lines, access tokens, mail or
tool arguments to `rail.log`; treat the log as sensitive if you do.

## Configuration

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

Unknown keys and wrong types fail to load, with the file's path and line number. Edits are picked
up by the running daemon within seconds; an edit that leaves the file invalid is logged and the
running syncs carry on unchanged.

### `[general]`

| Key | Default | Meaning |
|---|---|---|
| `secrets` | `"keyring"` | Where tokens are kept: `"keyring"`, `"file"` or `"1password"`. Change it with `rail secrets set` to move existing tokens. |
| `poll_interval` | `"60s"` | How often each account is checked for new mail. At least `5s`. |
| `history_days` | `0` | How many days of mail the store keeps; `0` is the whole mailbox. |
| `body_cache_days` | `30` | Bodies of threads with mail this recent are fetched ahead of time. |
| `icons` | `"unicode"` | `"unicode"` or `"ascii"`. The mail screen's glyphs; `"ascii"` swaps them for `*`, `+`, `@`, `ok`, `~`, `|`, `v`, `>` and `-` (the rules), for fonts without them. |
| `theme` | unset | A scheme name as `:theme` lists it (case does not matter; names may contain spaces). Checked when the mail screen starts; a misspelling names the nearest scheme. |
| `accent` | unset | The accent colour: a palette slot `0` to `15` of the scheme (a number, or a string such as `"5"`), a colour name such as `"blue"`, or `"#rrggbb"`. Without truecolor a `"#rrggbb"` accent is not applied. |
| `split_threshold` | `140` | At this many columns and wider the reader opens beside the list (in the mail screen). |
| `hints` | `true` | `false` turns the key hints off: the bottom row keeps only notices and each account's state, and the key boxes' rows go to the panes. `:hints` in the palette toggles it and saves it. |
| `clock` | `true` | `false` hides the time of day at the right edge of the header. `:clock` in the palette toggles it and saves it. |
| `time` | `"24h"` | `"24h"` writes times of day as `14:05`, `"12h"` as `2:05 pm` (the list, the header's `Synced` and its clock, the Drafts list, `:errors`, the reader's title block and cards (`04:32` today, `Thu 08:55` yesterday and this week), and the `Date:` header line `H` shows). `:time` in the palette toggles it and saves it. |
| `dim_read` | `"dot"` | Read threads: `"dot"` keeps the small muted dot, `"subject"` also dims the subject, `"all"` dims the sender, subject, count and time. `:dim-read` cycles the modes and saves the choice. |
| `thread_order` | `"oldest"` | How the reader lists a thread's older messages under the newest one: `"oldest"` first or `"newest"` first. `:thread-order` in the palette toggles it and saves it. |
| `layout` | `"auto"` | Only `"auto"` works in this version; `"split"` and `"stack"` are accepted and act as `"auto"`. |
| `editor` | unset | The command that writes mail, split like a shell command line (no shell runs), e.g. `"nvim"` or `"code --wait"`. Unset: `$VISUAL`, then `$EDITOR`, then `vi`. Read when the mail screen starts. |
| `bulk_confirm` | `50` | How many threads an archive, star or mark-read may cover before Rail asks first; `0` never asks. Trash always asks. |
| `attachment_dir` | unset | Where `s`, `S` and `w` save attachments. Unset is your download directory (the `XDG_DOWNLOAD_DIR` entry of `~/.config/user-dirs.dirs` on Linux, else `~/Downloads`). `~` is expanded, and refused when there is no `$HOME` to expand it to; a relative path is refused, as is one inside Rail's own attachment cache (`attachments` in the cache directory, reached directly, through a symlink or through `..`), whose files Rail sweeps a day after it writes them. |
| `saved_search_counts` | `true` | Unread counts beside each saved search in the sidebar. `false` leaves them out and skips the count queries, the one thing saved searches cost on a large mailbox. `:search-counts` toggles it and saves it. |
| `rich_html` | `true` | The reader draws HTML mail's bold, italic, quoted and linked text in those styles. `false` shows the same text plain. `:rich-html` toggles it and saves it. |
| `images` | `true` | With `rich_html`, the reader draws the pictures an HTML message carries inline, through the terminal's image protocol (kitty, iTerm2 or sixel) or as coloured half blocks elsewhere. Rail asks the terminal which it speaks as it starts, waiting up to 2 s for one that does not answer; with `TERM` unset, empty or `dumb` it does not ask and draws half blocks. `false` leaves them out. `:images` toggles it and saves it. |
| `remote_images` | `false` | With `images`, the reader fetches every HTML message's remote pictures, which tells their senders you opened the mail. `false` holds them back until `i` loads one thread's. `:remote-images` toggles it and saves it. |
| `remote_images_from` | `[]` | Senders, by address, whose remote pictures the reader always fetches, such as `["jira@example.com"]`: for mail whose From line the receiving server vouched for (DMARC, or DKIM aligned with the sender's domain), since a forged message can carry the address too. `I` on a message adds its sender or removes them, and saves the list. |
| `remote_images_ipv6` | `true` | Whether the daemon may fetch remote pictures over IPv6. `false` refuses every IPv6 address, for an IPv6-only network whose NAT64 prefix (which looks public) could reach private IPv4 space. Read at each picture, so a change applies without a restart. |

### `[[saved_searches]]`

One table per saved search, in the order the sidebar lists them. `S` on the mail screen writes
them, and so do `rename-search` and `remove-search`; your comments, formatting and other tables
survive each write, and a rename edits the table where it stands, so its row keeps its place.
A rename changes only the `name` line: the `query` stays exactly as the file has it, so a query
you edited by hand while Rail was open is kept.

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | 1 to 40 ASCII letters, digits, `-` or `_`; unique among saved searches, compared ignoring case. It is what the sidebar, the palette and the header show. |
| `query` | yes | Filter text, exactly as you would type it after `/`; not empty, at most 1024 characters. |

Rail keeps at most 20 saved searches. A name with a space or an accent in it, a duplicate name,
an empty or over-long query or a 21st table makes `config.toml` invalid, and `rail doctor` says which
entry is wrong — by name and position, never by quoting the query, which is yours. A query that
narrows nothing — only characters the filter ignores, such as `* ( )` — lists no threads and
counts none, rather than every thread of every account.

A Rail older than saved searches cannot read a `config.toml` that holds `[[saved_searches]]`
tables or `saved_search_counts` in `[general]`: it reports the config invalid. To go back to an
older version, remove them first.

```toml
[[saved_searches]]
name = "needs-reply"
query = "from:dana is:unread"

[[saved_searches]]
name = "big-attachments"
query = "has:attachment"
```

### `[[link_actions]]`

Mail from an issue tracker carries the record's id in its subject — `(#12345)`, `[PROJ-123]`,
`#4711` — so the way to the record is a substitution, not a search. One table per rule; `g t`
(`list.open_link`, in the list and the reader) runs every rule over the subject of the thread
under the cursor.

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | 1 to 40 ASCII letters, digits, `-` or `_`; unique among link actions, compared ignoring case. |
| `match` | yes | A regular expression over the subject. Its capture groups fill the template. |
| `url` | yes | An `http` or `https` template whose `{1}`, `{2}`, … are `match`'s groups, numbered from 1. |

Every match of every rule is offered, so a subject carrying two ids reaches either: one match
opens after the same confirmation as any other link, several are listed by number with the
rule that found each and the address it opens; on a narrow terminal an address loses its middle,
so the id at its end stays visible. One address is one choice, whether a subject
names the same record twice or two rules read it the same way. A subject no rule matches is a
notice, not an error. Rail keeps at most 20 link actions.

The subject is the sender's text, so a capture is never trusted. Each one is percent-encoded
before it reaches the template, which leaves it no `/`, `?`, `#` or `@` to steer the address
with. Encoding cannot defuse a dot segment, though — a parser reads `%2e%2e` back as `..` —
so a capture of `.` or `..` drops the match rather than climbing out of the path the rule
chose, and a template whose own path holds such a segment, written with dots or `%2e`, is
refused outright, since an
empty capture beside it would climb just the same. The address Rail then builds goes through
the same confirmation as a link in a message, with the warnings described under
[Reading mail](#reading-mail).

Everything before the path — the scheme, any credentials, the host and the port — comes from
this file and from nowhere else. To settle that, Rail fills the template twice with different
values and reads both with the parser that later opens the link, refusing the rule unless the
two agree. Reading the template any other way would let the check disagree with what the
address turns out to mean: `https:///{1}` and `https://\/{1}` both look like they name a
host, and a parser hands the host to the capture in each.

A name that breaks the charset, a duplicate name, a pattern that does not compile, a template
that is not `http`/`https`, one a match could steer before the path, one whose path holds a
`.` or `..` segment, one with no `{n}` at all or one naming a group the pattern does not have
makes `config.toml` invalid, and `rail doctor` names the rule.

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

### `[[accounts]]`

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
| `ai_send` | no | `"confirm"` (default), `"drafts"` or `"client"`; see below. |
| `signature` | no | A text file whose contents go under `-- ` at the end of every new message, reply and forward from this account (`~` is your home directory; trailing blank lines are dropped; at most 4 KiB are used). Read when the mail screen starts; a file that cannot be read is noticed when you start writing, and the message goes without it. |
| `from_name` | no | The name shown before `address` in `From:` of mail Rail sends (spaces allowed, at most 200 characters). |

`address`, `color` and `tenant` must not contain spaces, control characters or Unicode format
characters (such as U+202E); such a value is refused by `rail account add` and fails to load from
a hand-edited file. `from_name` may contain ordinary spaces but no other whitespace, control
or format characters.

### Sync settings

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

- A `body_cache_days` larger than a non-zero `history_days` is treated as `history_days`.
- Shrinking `history_days` removes local copies of older mail on the next pass (mail on the
  server is never touched). Growing it fetches further back, at most 2,000 messages a pass, while new mail
  keeps arriving within one `poll_interval`.
- Starred mail and threads with recent mail are never pruned.
- A first Gmail sync is paced to stay under Gmail's per-user quota, about 250 messages a minute:
  100,000 messages take about seven hours. Newest mail is fetched first. Mail that arrives during
  the first sync appears once it finishes; none is missed.
- An interrupted sync resumes where it stopped. If Gmail's history for the account expires (after
  a long time offline), Rail does a full resync, which reads every message again.
- A Microsoft 365 account is paced to Microsoft's limits, at most 900 requests a minute, and reads
  folder by folder. With a non-zero `history_days`, Microsoft returns at most 5,000 messages per
  folder, and growing `history_days` reads every folder again. See
  [Microsoft 365 mail](#microsoft-365-mail).

### `ai_send`: what an AI agent may do with your mail

```toml
[[accounts]]
name = "work"
# ...
ai_send = "confirm"
```

- `confirm` (default): Rail shows you the recipients, subject and start of the body in your MCP
  client's confirmation dialog and sends only on an explicit yes. A client without that dialog
  gets drafts-only behaviour.
- `drafts`: the agent may create and edit drafts, never send.
- `client`: Rail relies on your MCP client's own per-call approval prompt. The weakest option.

The policy will cover sending, replying, forwarding and deleting. The operations that save, send
and discard a draft, save one to the account's Drafts and import one from there exist (`draft.save`,
`draft.send`, `draft.discard`, `draft.push`, `draft.import`) and Rail's own terminal uses them, but they are not offered over MCP: no agent tool for mail exists yet, so today the key
is only recorded and reported (`rail doctor`, `rail_status`, `rail_accounts_list`).

### Tokens in 1Password

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

Rail keeps one item per mail account, titled `Rail: <address>` and tagged `rail`, with the tokens
in concealed fields. Values reach `op` only on its standard input, never as command-line arguments.

- **Turn on the 1Password app's CLI integration** (Settings → Developer → Integrate with 1Password
  CLI). The daemon can then refresh tokens however it was started. A session from `op signin` lives
  only in the shell that ran it and expires after about 30 minutes without use.
- **Moving tokens.** `rail secrets set 1password --vault <vault>` copies each account's tokens,
  reads them back to verify, updates `config.toml`, and only then deletes the old copies. A failure
  part way leaves the old copies in charge. The same command moves them back to `keyring` or `file`.
- **Client secret in 1Password.** Give `client_secret = "op://Rail/Google client/client_secret"`
  (or pass an `op://` reference to `--client-secret`), or let Rail store it with
  `rail account add ... --client-secret <secret> --store-secret-in-1password`. Rail reads it with
  `op read` only when a login needs it and never caches or logs it. This works with any token
  backend.
- **`op_path`** is only ever read from `config.toml`. Rail refuses it when it is relative, not an
  executable, owned by another user, or writable by anyone but its owner, or when a directory above
  it is. A Homebrew-on-Linux `op` usually fails this check; prefer 1Password's own package (for
  example `/usr/bin/op`).

### File locations

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

Override any location with `RAIL_CONFIG_DIR`, `RAIL_DATA_DIR`, `RAIL_CACHE_DIR`, `RAIL_STATE_DIR`
and `RAIL_RUNTIME_DIR` (the directory that holds the daemon's socket). Give each setup its own
`RAIL_RUNTIME_DIR` to run two side by side.

### The background daemon

Every operation (logins, saves, sync, search, `rail doctor`) runs in one background process, so two
commands never race on the store, the config or a login. The TUI's own config saves (`:theme`,
`:time`, `:hints` and the like) take the same config lock, so no edit is lost. You never start it
yourself: the first
command that needs it starts `rail daemon run` in the background, or your login does after
`rail daemon install`. While any account is configured it keeps running and syncs; with none it
exits after ten minutes idle. It also puts snoozed threads back at their time, so a snooze comes
back on time while the daemon runs (from your login, after `rail daemon install`); one whose time
passed while no daemon ran comes back as soon as one starts. A login you begin keeps waiting in the daemon until you finish it,
cancel it, or 15 minutes pass, and opens the browser on the display of the command that began it.

The daemon keeps the environment of the command that started it (notably `PATH` and any
`OP_SESSION_*`). Run `rail daemon stop` to have the next command start one with your current
environment. A command whose `RAIL_*_DIR` settings differ from the running daemon's is refused with
"a Rail daemon is already running with different directories".

A client replaces a daemon from an older version *or an earlier build of the same version* (each
build carries the time `rail-core` was last built; a `SOURCE_DATE_EPOCH` exported in the build
environment replaces that time, so exporting it permanently makes every build look alike), so
reinstalling `rail` while a daemon runs is enough. A program that is itself older than the running
daemon, such as a long-lived `rail mcp` after an upgrade or rebuild, never stops its successor: it
says "Rail was upgraded; restart this program". After a rebuild the hint adds that
`rail daemon stop` lets this program start a daemon of its own build.

The login unit written by `rail daemon install` runs the exact `rail` binary you installed it with
(install again after moving it) and pins the `RAIL_*` directories your shell resolved at install
time. It restarts the daemon if it fails, at most five times in five minutes, and not after a clean
`rail daemon stop`. It sets `PATH` to the system directories plus the directory of `op_path` if you
set one, and on Linux unsets display, browser, `RUST_LOG` and 1Password variables inherited from the
user service manager. `systemctl --user show-environment` lists what your user manager can still
pass in.

## Commands

| Command | What it does |
|---|---|
| `rail` | Opens the mail screen when an account is configured, otherwise the setup wizard. Both need a terminal: in a pipe they print a hint and exit 2. A bad `config.toml` or `[keys]` entry is reported with its line before the terminal changes (exit 1). |
| `rail setup` | Opens the setup wizard to add an account. |
| `rail account add [gmail\|m365]` | With no flags, opens the wizard (with that provider chosen, if given). |
| `rail account add <gmail\|m365> --address <addr> --client-id <id> [--client-secret <secret>] [--tenant <tenant>] [--name <name>] [--color <colour>] [--store-secret-in-1password]` | Adds an account without the wizard, then opens the browser login. A partial set of flags exits 1 and names the missing one. |
| `rail account login <account>` | Logs in again to an existing account (name or address) and replaces its tokens; `config.toml` is untouched. |
| `rail account list` | Lists accounts and whether their tokens are stored, without reading the tokens. |
| `rail account remove <account>` | Removes the account's config entry and tokens; its mail stays in the store. Asks `Remove? [y/N]`; refuses (exit 1) when stdin or stdout is not a terminal. |
| `rail secrets set <keyring\|file\|1password> [--vault <vault>] [--account <account>] [--no-migrate]` | Changes where tokens are kept and moves them there (`--no-migrate`: setting only; each account then needs `rail account login`). Asks `Change? [y/N]`; interactive only. |
| `rail sync [<account>] [--no-wait]` | Wakes the sync for one account or all and shows progress until caught up. Exits 0 when caught up, 1 if a sync failed, 2 if an account needs `rail account login`, 130 on Ctrl-C. If nothing changes for 10 minutes it stops watching (the sync goes on). If the daemon restarts, or restarts its sync, during the watch, it says which and waits for the new pass. `--no-wait` wakes it and exits 0. |
| `rail search <query> [--account <account>] [--limit N]` | Searches the local store and each account's mailbox on its server (Gmail or Microsoft 365). One line per thread: date (UTC), account, sender, subject; never a snippet or body. `--limit` is 1 to 100 (default 20). Results go to stdout, notes to stderr. Exits 1 for an unknown account, or when nothing was found and a server search failed. |
| `rail doctor` | Checks config, store, token backend, each account's tokens, `ai_send` and sync, the daemon and the login unit; notes a `general.layout` this version treats as `auto`, and prints a `theme` line when `general.theme` is set. Exits non-zero for config, store, token-backend and token-read errors, and for an account that needs a login or whose sync failed for good; warnings and the login-unit line do not change the status. |
| `rail daemon status` | Shows whether the daemon runs, with pid, version and build, load and socket. Exits 1 when none runs; never starts one. |
| `rail daemon stop` | Asks the daemon to exit once any save in flight is written; waits up to 35 seconds. |
| `rail daemon run` | Runs the daemon in the foreground (Ctrl-C or SIGTERM stops it cleanly). Rarely needed. |
| `rail daemon install` | Writes and enables a systemd user unit (Linux) or launchd agent (macOS) for the user running it that starts the daemon at login; stops a daemon already running first. If enabling fails it puts the previous state back and exits 1; if the daemon does not answer within 10 seconds it exits 1 with the unit's state (the unit stays installed). |
| `rail daemon uninstall` | Stops, disables and removes the login unit. |
| `rail mcp` | Serves Rail's tools to an AI agent over MCP on stdin and stdout. |

## Troubleshooting

Start with `rail doctor`. Every error Rail prints is followed by a line starting `hint:` that says
what to do next; words from outside Rail (a provider's or the system's) are quoted on a line of
their own.

| Symptom | Fix |
|---|---|
| A token refresh fails with "refresh token rejected", or `rail doctor` shows tokens `missing` | `rail account login <name>`. Common after a revoked grant, a sign-in frequency policy, or tokens deleted from the keyring. |
| `the signed-in mailbox does not match this account's address` | Log in again and select the account's mailbox. For Google Workspace, configure its primary mailbox address rather than a custom alias. Existing credentials are kept. |
| Gmail asks you to log in again every week | The OAuth consent screen is still in "Testing"; publish it ("In production"). |
| No Secret Service (tty-only Linux, containers) | Set `secrets = "file"` under `[general]` before adding accounts, or move existing tokens with `rail secrets set file`. |
| "1Password is locked" or `op` not signed in | Unlock the app, or turn on its CLI integration; `rail doctor` checks with `op whoami`. |
| `no operation named "…"` | The daemon is from an earlier build. Rail now replaces an older build by itself; if you still see this, run `rail daemon stop`. |
| `rail doctor` fails because the daemon cannot start, e.g. the socket path is too long | Set `RAIL_RUNTIME_DIR` to a shorter directory. |
| "a Rail daemon is already running with different directories" | Run `rail daemon stop` from the shell that started it, or use the same `RAIL_*` settings. |
| "Rail was upgraded; restart this program" | An older `rail mcp` or wizard is still open after an upgrade; restart it. After a rebuild, if restarting does not help, this `rail` is the older build: run `rail daemon stop` (or use the newer binary). |
| `rail daemon install` says no user service manager answers | Some containers have no `systemctl --user`; the daemon still starts on demand. |
| `rail doctor` warns that the login unit does not match this binary | You moved or reinstalled `rail` elsewhere; run `rail daemon install` again. |
| A first sync is taking hours | Expected for a large mailbox; set `history_days` to sync only recent mail. `rail doctor` and `rail_sync_status` show progress. |
| Colours look wrong or a theme does nothing inside tmux | Add `set -as terminal-features ",xterm-256color:RGB"` (tmux 3.2+) or `set -ga terminal-overrides ",*:Tc"` to `~/.tmux.conf`, and make sure `COLORTERM=truecolor` reaches the shell inside tmux. |
| `Microsoft 365 refused access: an administrator may need to grant consent (see "Work or school accounts and admin consent" in the README)` | An administrator grants consent, as in [Work or school accounts and admin consent](#work-or-school-accounts-and-admin-consent), then run `rail account login <name>` if the login also fails. |
| `this mailbox cannot be reached through Microsoft Graph (on-premises, unlicensed or inactive)` | The mailbox must be an Exchange Online mailbox with a licence. |
| `Outlook uses folders, not labels: press m to move.` | Expected: Outlook has folders, not labels. Press `m` to move the thread to a folder. |
| `This mailbox has no Archive folder; press m to move.` | The mailbox has no Archive folder and Rail never makes one. Press `m` to move the thread to a folder. |
| `This mailbox has no Deleted Items folder.` | `d` found no Deleted Items folder in the last folder listing, which an Exchange Online mailbox always has; Rail put the thread back. Press `Ctrl-r` to sync, or `m` to move the thread to a folder. |
| `Could not …: Rail holds no messages of this Microsoft 365 conversation` | The thread's messages left the store before the action was sent (another device moved or deleted them). Press `Ctrl-r`; the next sync shows where they are now. |
| `a folder cannot be synced: its sync link is longer than 4 KiB` (in the header as `⚠ name sync failed`; `:errors` names the folder after it, as `(from outside Rail (the provider or the system): "Projects")`) | Microsoft answered three times in a row with a sync link for that folder longer than Rail keeps. Rail stops syncing that folder until the daemon restarts (`rail daemon stop`); every other folder goes on, and the mail Rail holds from it stays. Until the restart, or until the folder is deleted in Outlook, every pass counts as failed: `rail doctor`'s last-success time stops advancing and its sync check fails, and `rail sync` exits 1. Mail deleted on the server while the folder is given up on stays in Rail: a full resync (when Microsoft expires a sync link, a synced folder is deleted, or `history_days` grows) that runs while the folder is still given up on removes nothing, and a restart alone does not remove it either. It goes with the first full resync after a restart. Please report it if it comes back. |
| `the folder for this move no longer exists in Microsoft 365` | The folder was deleted in Outlook before the move was sent; Rail put the thread back. Press `m` and pick another folder. |
| `Microsoft 365 is busy or unavailable (HTTP 429)`, or `retrying` in `rail sync` | Microsoft is throttling the mailbox. Rail waits as long as Microsoft asks (up to two minutes at a time) and tries again. |
| `Gmail no longer has this draft` | The draft was deleted in Gmail since Rail last synced, and its row goes after the next sync. A draft you already opened stays on this computer, and `s` saves it to Gmail again. |
| `Microsoft 365 no longer has this draft` | The draft was deleted in Outlook since Rail last synced, and its row goes after the next sync. A draft you already opened stays on this computer, and `s` saves it to Outlook again. |
| `draft: saving to Gmail, try again in a moment` | A save of this draft to Gmail is still on its way, so it cannot be discarded yet. Wait for `saving to Gmail…` to leave the status bar and press `d` again. |
| `draft: saving to Microsoft 365, try again in a moment` | A save of this draft to Outlook's Drafts is still on its way, so it cannot be discarded yet. Wait for `saving to Microsoft 365…` to leave the status bar and press `d` again. |
| A reply from Microsoft 365 is not in the Outlook conversation | The message it answered was gone from Outlook for good (Deleted Items emptied) before the send, so Rail sent the reply as a new message. |
| Two copies of one draft in Gmail's Drafts | Creating a Gmail draft is tried once; a failure that hid a success can leave a second copy. Delete the extra one in Gmail. |
| The login URL did not open in a browser | Select it on screen and open it while Rail is still running. |
| The editor opens and Rail comes straight back with the message unchanged | A graphical editor that returns at once. Give it its wait flag in `general.editor`, e.g. `editor = "code --wait"`. |
| `cannot run "vi"` (or your editor's name) when writing | No editor was found: `general.editor`, `$VISUAL` and `$EDITOR` are unset or name a program that is not installed, and `vi` is missing. Set `general.editor`. |
| The editor's text is still on screen after it closes | Rail could not take the terminal back from the editor. It repaints the whole screen on the next frame and logs `could not take the terminal back from the editor` in `rail.log`. |
| `Could not send: Gmail refused the request (HTTP 400)` | Gmail refused the message and did not say why. The review opens again: check the addresses with `t` and send again. |

### Microsoft sign-in errors

| Code | Meaning | What to do |
|---|---|---|
| `AADSTS65001` | Nobody has consented to Rail's permissions yet. | An administrator grants admin consent; non-administrators send them the URL the wizard shows. |
| `AADSTS90094` | Your organisation requires an administrator to approve apps. | Same: send the admin-consent URL, then retry with `r`. |
| `AADSTS50076`, `AADSTS50079` | MFA or another policy step is required. | Complete the prompt in the browser; if it keeps failing, ask your administrator. |
| `AADSTS50194` | A single-tenant app was used with the `common` endpoint. | Set the tenant to your tenant ID or domain. |
| `AADSTS50020` | The account belongs to a different tenant than the app. | Use an account from the app's tenant, or register the app for multiple organisations. |
| `AADSTS700016` | The application is not found in that tenant. | Check the client ID (Application (client) ID, not Object ID) and the tenant. |
| `AADSTS7000218` | Microsoft expected a client secret. | Enable "Allow public client flows" under **Authentication**. |
| `AADSTS50011` | The redirect URI does not match the registration. | Add `http://127.0.0.1` under **Mobile and desktop applications**. |
| `AADSTS53003` | A Conditional Access policy blocked the login. | Ask your administrator; the policy, not Rail, decides. |

## Contributing

Bug reports and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for the
development loop, ground rules and commit format, and [CHANGELOG.md](CHANGELOG.md) for what has
shipped.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

The colour schemes behind `:theme` are the
[iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes) collection,
© 2011 to present Mark Badolato, MIT licence.
