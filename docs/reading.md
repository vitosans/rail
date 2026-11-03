# Reading mail

The mail screen: the thread list, the sidebar, the header and status bar, the reader, pictures,
links and attachments, filtering and saved searches, the command palette, and themes.

- [Opening the mail screen](#opening-the-mail-screen)
- [The thread list](#the-thread-list)
- [The sidebar](#the-sidebar)
- [The header](#the-header)
- [The status bar](#the-status-bar)
- [The reader](#the-reader)
- [Pictures](#pictures)
- [HTML mail](#html-mail)
- [Links](#links)
- [Attachments](#attachments)
- [Opening a message in the browser](#opening-a-message-in-the-browser)
- [Older messages](#older-messages)
- [Filtering the list](#filtering-the-list)
- [Saved searches](#saved-searches)
- [The command palette](#the-command-palette)
- [Settings in the palette](#settings-in-the-palette)
- [Help](#help)
- [Sync and connection state](#sync-and-connection-state)
- [Themes](#themes)

Every key here can be changed; see [Keys](keys.md). What you can do to a thread (archive, trash,
labels, snooze) is in [Doing things to mail](actions.md).

## Opening the mail screen

`rail` with an account configured opens the mail screen. It needs an interactive terminal; in a
pipe it prints a hint and exits 2.

It reads the local store at once, and connects to the background daemon (starting it if needed)
for sync progress.

- If the store is not set up yet, the list says so and points to `rail sync`.
- If the daemon cannot be reached, the header says so (`daemon unreachable (Ctrl-r retries)`) and
  `Ctrl-r` retries.

## The thread list

The thread list shows the unified inbox, newest first. `1` to `9` show one account's inbox, `0`
the unified inbox again.

### Date groups

Threads sit in date groups, each name on its own row above its threads:

| Group | Holds |
|---|---|
| `TODAY` | Today's mail. Mail dated after today is here too, with its date, as is a date too far ahead to read. |
| `YESTERDAY` | Yesterday's mail. |
| `THIS WEEK` | Two to six days ago. |
| `OLDER` | Everything earlier, and a date too far behind to read. |

The group of the top row stays on the first row as you scroll. A group's name never sits alone on
the bottom row.

### Paging

`PgDn` and `PgUp` (`Ctrl-d`, `Ctrl-u`) move a page by the threads on screen:

- the next page starts with the thread after the last one shown;
- the page above ends with the thread before the first one shown;
- the cursor keeps its row, or goes to the page's last thread when the page holds fewer.

### What a row shows

From left to right:

| Part | Unicode | `icons = "ascii"` |
|---|---|---|
| Unread dot, in the account's colour, while the thread has unread mail | `●` | `*` |
| Muted dot, once it is read | `·` | `.` |
| Selected with `Space` | `▸` | `>` |
| The account's badge (its label's first letter on its colour), in `All inboxes`, `Starred`, `Sent` and `Trash` | | |
| The people | | |
| Starred | `★` | `+` |
| Attachments | `📎` | `@` |
| The subject, with the message count after it (`(3)`, never cut) | | |
| At the right edge, the time | | |

Unread threads are bold. How read threads look is set by `dim_read`; see
[`:dim-read`](#dim-read).

### Times in the list

| When | Shown |
|---|---|
| Today and yesterday | `14:05` |
| This week | `Mon 18:20` |
| Earlier this year | `Oct 03` |
| Before this year | `2024-10-03` |

With `time = "12h"` (or `:time`) the times read `2:05 pm` and `Mon 6:20 pm`, and the time column
is two cells wider.

## The sidebar

The sidebar is shown at 100 columns and wider. `b` toggles it and `Tab` moves into it. It has two
sections:

| Section | Holds |
|---|---|
| UNIFIED | `All inboxes`, `Starred`, `Drafts`, `Sent` and `Trash`, each with every account's mail. `Snoozed` too, while anything is snoozed ([Snooze](actions.md#snooze)). |
| ACCOUNTS | Each account (by its `label`) with its system folders under the provider's names, then `More folders (N)`. |

Saved searches have rows of their own under `SEARCHES` ([Saved searches](#saved-searches)).

The system folders are:

- Gmail: Inbox, Sent, Drafts, Spam and Trash.
- Microsoft 365: Inbox, Sent Items, Drafts, Archive, Junk Email and Deleted Items.

Every other folder or label (up to 500 per account) is under `More folders (N)`, as a tree of its
`/`-separated path (`Inbox/Infrastructure/Linode`), with Gmail's categories under `Categories`.

### Opening and folding

`Enter` opens the mailbox under the cursor:

- an account opens its inbox;
- on Microsoft 365, a top-level path named after a system folder, like `Inbox` above, opens that
  folder;
- `More folders` and any other path that is not a folder itself unfold instead, as does Gmail's
  `Sent` path of a label `Sent/2024`, which is not inside Sent.

`Space`, `←` or `→` folds or unfolds an account, `More folders` or a path. Accounts start
unfolded and the rest folded, for the session.

### Counts, badges and icons

Counts are unread threads (on a folded path, everything under it), drafts on `Drafts`, and none on
`Sent` and `Trash`. They sit on the right, with thousands separators.

Each account row has its fold arrow and a badge, its label's first letter on the account's colour.

| Item | Unicode | `icons = "ascii"` |
|---|---|---|
| Unfolded / folded | `▾` / `▸` | `v` / `>` |
| Inbox | `✉` | `i` |
| Starred | `★` | `*` |
| Drafts | `✎` | `d` |
| Sent | `➤` | `>` |
| Trash | `✕` | `x` |
| Archive | `▣` | `a` |
| Spam | `⊘` | `s` |
| `More folders` and a path with folders under it | `📁` | `/` |

While the sidebar does not have focus the current mailbox's row is in the accent colour. The
sidebar scrolls to keep it (or, while the sidebar has focus, the cursor) in view.

## The header

The top of the screen is the header:

```
▍Rail │ All inboxes · 2 accounts · 22,853 unread
```

It has three parts.

**The scope**, on the left:

- `Starred · 2 accounts · 12 unread` and `Sent · 2 accounts` in the merged mailboxes;
- an account's own Sent, Trash or Drafts shows no unread count: `work · Sent`;
- with threads selected, it ends with `3 selected`.

**The search region:** `│ 🔍 / Search` (`| / Search` with `icons = "ascii"`). The filter shows there
once set. The Drafts folder has none.

**The summary**, on the right: one line for every account's sync, then the time of day at the right
edge: `● Synced 14:47 │ 15:02` (`3:02 pm` with `time = "12h"`; `:time` changes it at once).

- `● Synced 14:47` when all are caught up (the earliest account's time);
- else the worst state: `⟳ Syncing work`, `⟳ Backfill 1,200/5,000`, `⟳ retrying`,
  `work stopped`, `! 2 failed`, `! work needs login`, `! work sync failed`, or the daemon's
  state.

`clock = false` under `[general]`, or `:clock`, hides the clock.

### Boxes

At `split_threshold` (140 columns by default) and wider:

- the sidebar, the list and the reader are boxed, the focused pane in the accent colour;
- with `icons = "ascii"` the boxes are drawn with `+`, `-` and `|`, and an unboxed header reads
  `|Rail | All inboxes …`;
- at least 12 rows high, the header has a box of its own too (never in the accent colour), and
  inside it the row reads `│ Rail │ All inboxes … ● Synced 14:47 │ 15:02 │`, without the `▍` bar
  (`| Rail | …` with `icons = "ascii"`).

### Narrow terminals

- When the scope and the summary do not both fit, the summary and the clock stay whole and the
  scope is cut.
- While a filter is set or being typed, the search slot keeps at least 24 cells (at 60 columns and
  wider; less only when the summary itself leaves no room): the scope is cut first.
- Under 60 columns the clock and the search region go, and under 40 the summary.

## The status bar

The status bar at the bottom names the keys of the pane you are in
(`Enter open  j/k move  Tab sidebar  r reply  …  ? help`, with the keys your `config.toml` sets),
and shows each account's state on the right.

Narrower than `split_threshold` it is one row. At `split_threshold` and wider (and at least 12
rows high) it takes three rows and each key is drawn in its own box, with no frame around the bar
(`+`, `-` and `|` with `icons = "ascii"`):

```
┌───────┐      ┌─────┐      ┌─────┐         ┌───┐
│ Enter │ open │ j/k │ move │ Tab │ sidebar │ ? │ help        ● work ✓ · ● home ✓
└───────┘      └─────┘      └─────┘         └───┘
```

`:hints` (or `hints = false` under `[general]`) turns the hints off: the bar is one row of notices
and account states at every width, and `?` still lists the keys.

Notices show on the status bar, in place of the key hints. The account states are described in
[Sync and connection state](#sync-and-connection-state).

## The reader

`Enter` opens a thread. At `split_threshold` columns (140 by default) and wider the reader opens
beside the list; narrower, it covers the screen. `q` or `Esc` closes it.

### The title block

The reader starts with the thread's title block:

1. the subject;
2. the newest message's sender, its time and the account's badge and label
   (`Dana Reyes  04:32   W  WORK`);
3. its recipients by name and the thread's size (`To: Vito, Tom  /  4 messages`, or
   `/  4 messages` when it names none);
4. a rule.

| When | Time shown |
|---|---|
| Today | `04:32` |
| Yesterday and this week | `Thu 08:55` |
| Before | `Oct 03` or `2024-10-03` |

### Messages and cards

The newest message is open under the title block. The older messages follow as cards between
rules, oldest first (`thread_order = "newest"` under `[general]` runs them newest first).

A card is the message's number in the thread in a grey box, its sender (bold while unread, `★`
when starred) and its time at the right edge (`▸   2   Tom` … `Mon 22:11`), over a line of its
text.

When a long thread's oldest messages are not shown,
`[5 older messages of this thread are not shown]` stands where they would be.

With `icons = "ascii"` the arrows are `>` and `v` and the rules `-`.

### Moving in the reader

| Key | Action |
|---|---|
| `j` / `k` | Move the cursor. |
| `n` / `p` | Jump between the newest message and each card. |
| `Tab` (or `Enter` on a card, the subject or a preview) | Open or close the message under the cursor. |
| `Enter` on `[n quoted lines]` | Show the quoted text. |
| `H` (`h` before snooze took it) | Show or hide, for the session, the headers of every open message. |

The headers are `From:`, `To:` and `Cc:` with addresses, `Date: Fri 10 Oct 2025 12:40`, and
`Subject:` when it differs from the thread's.

## Pictures

The pictures a message carries itself are drawn as soon as it opens.

Rail asks the terminal at start-up how it draws pictures (kitty, iTerm2 or sixel) and uses half
blocks when the answer is none of those. It does not ask, and uses half blocks, when:

| Case | Why |
|---|---|
| `TERM` is `dumb` or unset | The terminal cannot answer. |
| The terminal answers nothing within 2 s | An unanswered query would keep reading keys (#155). |
| Inside tmux with no client attached | tmux passes the query on to an outer terminal that is not there. |

Start Rail in an attached tmux session to get full pictures there.

### Remote pictures

**Remote pictures**, the ones an HTML message loads from the web, are held back, because fetching
one tells its sender you opened the mail.

- The message starts with
  `Remote pictures are not shown; i loads them, I always for this sender.`
- A remote picture with alt text shows `[image: …]` in its place.
- `i` loads the open thread's remote pictures. It loads every remote picture of the thread, a
  tracking pixel too when the mail does not declare its size.
- `remote_images = true` under `[general]`, or `:remote-images`, loads them in every thread.

### Trusting a sender

`I` loads them and remembers the sender of the message under the cursor, by address, so their mail
shows its pictures from then on (`remote pictures from jira@example.com: always`, saved as
`remote_images_from` under `[general]`). `I` on their mail again stops that (`…: ask first`).

A From line can be forged, so a trusted sender's mail loads its pictures by itself only when the
receiving server vouched for the sender's domain:

- Gmail's or Microsoft 365's own `Authentication-Results` show DMARC passed for it, or
- a DKIM signature of it (or of a parent domain) passed.

Other mail from that address waits with the hint, as an untrusted sender's does. So does mail
stored before Rail kept these results, until its body is fetched again; `i` loads it.

### How remote pictures are fetched

The daemon fetches them, never the terminal:

| Rule | Value |
|---|---|
| Scheme and address | Only over `https`, from public addresses |
| Cookies and referrer | None |
| Size | At most 5 MiB each |
| At once | At most eight |
| Kept | For a week, in Rail's private image cache |
| Cache size | At most 512 MiB (the oldest pictures go first) |

On an IPv6-only network with NAT64, the gateway's own prefix looks public but can reach private
IPv4 addresses, which Rail cannot tell. `remote_images_ipv6 = false` under `[general]` fetches
remote pictures over IPv4 only.

## HTML mail

HTML mail is shown as Rail's own rendering of it, the same text Rail stores and searches:

- its bold and headings bold, its italics italic, its quotes dimmed and its links underlined
  (`rich_html = false` under `[general]` or `:rich-html` shows that text without the styles);
- with its pictures, at the size it asks for;
- with each link as a numbered marker such as `[1]`.

Pictures in HTML mail:

- `[image: …]` shows until they arrive, and for any past the first 32 in an open thread.
- Pictures of a few pixels, such as tracking pixels, are left out.
- Mail stored by a Rail older than 0.4.0 gains its inline pictures the first time it is opened,
  with one more fetch of that message.
- `images = false` or `:images` leaves them all out.
- How they are drawn depends on the terminal; see `images` in
  [Configuration](configuration.md#general).

## Links

In terminals that support hyperlinks the link markers are clickable: iTerm2, WezTerm, kitty,
Ghostty, VS Code, Windows Terminal, Hyper, foot, Alacritty, GNOME Terminal and other VTE terminals;
not inside tmux or screen. While a box such as help or a confirmation is open the markers are
plain text.

`g x` opens a link: the marker on the cursor line, or the number you type when the line has none
or several.

### The confirmation

The confirmation shows where the link really goes before the address (cut if it is long):

- the host (an international or look-alike host in its ASCII `xn--` form), or
- for `mailto`, the recipients (`To`, `Cc` and `Bcc`, including those added by the link's
  parameters): up to five, then a count of the rest.

It warns when:

- the address has text before an `@` that could pass for a host
  (`https://bank.com@evil.example` goes to `evil.example`);
- a `mailto` link adds recipients;
- a `mailto` link carries a parameter other than `to`, `cc`, `bcc`, `subject` and `body` (some
  mail clients have acted on `attach=`). The warning names up to five such parameters.

Rail opens only `http`, `https` and `mailto` links of at most 2,048 bytes.

## Attachments

`a` lists the thread's attachments (name, type, size) with a cursor. Each message's rows sit
under a `MESSAGE <n>` heading when more than one message carries attachments.

| Key | Action |
|---|---|
| `j` / `k` | Move the cursor. |
| `Enter` or `o` | Open the highlighted one with the program your system uses for it. |
| `s` | Save the highlighted attachment. |
| `S` | Save every attachment of its message. |
| `Esc`, `q` or `a` | Close the list. |
| `w` (from the reader itself) | Save the cursor message's attachments without opening the list. |

### Saving

Saves land in `[general] attachment_dir` ([Configuration](configuration.md#general)), your
download directory by default. They never replace a file you have: a name already there becomes
`2-name.pdf`.

### Opening

Rail asks once a session before the first open, and only for the common document and image types
it hands over. Anything else is saved instead and says so.

Office documents (Word, Excel, PowerPoint and their OpenDocument kin) and RTF are among them, so
the question warns that they can hold macros or links that run or fetch content when opened.

A type is the sender's claim, and your system's opener may judge a file by its contents instead.
So before writing the copy Rail checks that the file starts the way its type does:

| Type | Must start with |
|---|---|
| PDF | `%PDF-` |
| Image | The PNG, JPEG, GIF, WebP, BMP, TIFF or HEIC signature |
| `.docx` and its kin | A ZIP whose first part is an Office one (so a Java archive renamed `.docx` is refused) |
| `.odt`, `.ods` and `.odp` | A ZIP naming its own type first |
| `.doc`, `.xls` and `.ppt` | The old Office header |
| RTF | `{\rtf` |

Plain text, CSV and TSV may be in any 8-bit encoding (UTF-8, or Latin-1 from a spreadsheet), but
must not:

- hold a NUL byte;
- start (past any byte-order marks and white space) with markup, a `#!` script line or a PDF or
  PostScript header;
- have an HTML, SVG or XML tag start in their first 64 KiB, whatever its case or the white space
  inside it.

A file that fails is not opened (a web page labelled as a picture would otherwise open in your
browser) and is saved instead, with a line that says so.

Where the opened copies live, and when they are removed, is in
[Security and privacy](security.md#what-is-stored-locally).

## Opening a message in the browser

`o` opens the message's original HTML in your browser, for a Microsoft 365 account too (the HTML
Rail stored).

- Rail writes it to a private (0600) file in the `open` folder of its cache directory
  (`~/.cache/rail/open/` on Linux; `RAIL_CACHE_DIR` overrides the cache directory), with scripts,
  frames and connections blocked.
- It removes the file when you close the reader, quit Rail, close the terminal window or stop Rail
  with `kill`. Files a crashed session left behind are removed the next time the mail screen
  starts, once they are a day old.
- The first time in a session Rail asks first, because **remote images, styles, fonts and media
  in the message load in the browser**, which can tell the sender you opened it. Rail itself never
  loads them.
- A message whose body is not stored yet is fetched first.
- A message with no HTML part says so: its text is all there is.

### Two limits of `o`

- Firefox installed as a snap (the default on Ubuntu) cannot read hidden folders such as
  `~/.cache`, so it shows a file-not-found page. Set `RAIL_CACHE_DIR` to a folder it can read (for
  example `RAIL_CACHE_DIR=$HOME/rail-cache`) or use a browser that is not a snap.
- A `<meta http-equiv="refresh">` tag in the message can still send the page to another address
  once it is open. The blocking above cannot stop it, so close the tab if the page moves.

## Older messages

Bodies of recent mail (`body_cache_days`) are already in the store. An older message shows
**fetching…** while the daemon fetches its body from its account's server (Gmail or Microsoft 365).

The rest of the thread stays readable, and the text appears when it lands. If the fetch fails the
message says why.

## Filtering the list

`/` opens the filter line in the header's search slot and filters the list as you type. `Enter`
keeps the filter; `Esc` clears it.

The slot is not drawn when the terminal is narrower than 60 columns, or, before a filter is set,
when fewer than 8 cells are left for it after the counts and the summary (with a filter set, only
when the summary itself leaves no room). The line then opens in the status bar.

### What the filter matches

Plain words search subjects, the names and addresses of senders and recipients (To, Cc and Bcc)
and stored bodies. These narrow it:

| Operator | Matches |
|---|---|
| `from:` | The sender |
| `to:` | The recipients, also Cc and Bcc |
| `subject:` | The subject |
| `has:attachment` | Mail with attachments |
| `is:unread` | Unread mail |
| `is:starred` | Starred mail |
| `before:YYYY-MM-DD`, `after:YYYY-MM-DD` | Local dates, by when the mail arrived |
| `account:<name>` | One account |

- Quote values with spaces (`from:"Dana Reyes"`).
- A thread matches when one of its messages matches all of them.
- Anything else is searched as words. The header's search slot shows how Rail read your filter
  (`/ is:unread · words: form:dana`), so a typo such as `form:dana` shows up as a word.

The filter searches the local store only (`rail search` also asks each account's server; see
[Commands](commands.md#rail-search)).

## Saved searches

`S` saves the filter in force under a name, so you never type it again.

- The prompt asks for a name of 1 to 40 letters, digits, `-` or `_`. `Enter` saves it and `Esc`
  cancels.
- A filter that narrows nothing (only characters the filter ignores, such as `* ( )`) is not
  saved. Rail says `That filter narrows nothing; add a condition to save it.`

### How a saved search behaves

A saved search then behaves like a mailbox: its own row in the sidebar under `SEARCHES`, an unread
count, a `Search: <name>` palette entry, a header title, paging and `Esc`.

- It lists every folder of every account narrowed by that filter, so it finds archived mail an
  inbox view would not.
- `/` narrows a saved search further. `Esc` on the reopened filter line clears only that live
  filter; it never removes a saved search.
- `S` inside an open saved search saves only what you typed after `/`, not that text joined to the
  open search's query. To save the narrower search, type the whole filter.
- An `account:<name>` inside a saved query still narrows to that account.
- A saved `before:`/`after:` keeps the date you typed, so a saved date ages: a search for
  `after:2026-01-01` means that one day for ever, not "since a year ago".

### Renaming and removing

On a saved search's sidebar row, `r` renames it and `d` removes it (asking first). They always act
on the search the prompt or the question named. If the list changed while one of them was open (an
earlier save or removal was answered underneath it), Rail says
`The saved searches changed; try again.` and changes nothing.

Rail keeps at most 20 saved searches, in `[[saved_searches]]` in `config.toml`, where they are
yours to read, edit and share; see [Configuration](configuration.md#saved_searches).

On a large mailbox each counted search costs a query of its own on every sidebar refresh, so
`:search-counts` turns the counts off.

## The command palette

`:` opens the command palette. Type a few letters and press `Enter`. The key bound to each command
is shown beside it. It lists:

- the key actions for the current view;
- accounts;
- labels, as `Label: AXON · Inbox/Infrastructure/Linode`: the account's label and the folder's
  whole path, folded away in the sidebar or not;
- the merged mailboxes (`Mailbox: Starred`, `Mailbox: Drafts`, `Mailbox: Sent`,
  `Mailbox: Trash`): that mailbox of every account together, newest first;
- each saved search as `Search: needs-reply`, and beside it a `Rename search: needs-reply` and a
  `Remove search: needs-reply` entry, which open the same prompt and the same question as `r` and
  `d` on its sidebar row.

### `:errors`

`:errors` lists the session's recent errors.

- Every sync pass that failed for good (one a retry will not mend) is listed there once, after its
  account's name, while `:errors` still holds it.
- While one of them is an action Gmail refused, `j`/`k` or `↓`/`↑` highlight an entry and `Enter`
  sends that action again.

### Saved-search commands

- `save-search` names the filter in force and saves it, exactly as `S` does. It is offered only
  while a filter is set.
- `rename-search <name>` and `remove-search <name>` rename or remove one saved search. There is
  one entry each, per search, so the palette says which. They are offered only once there is one.

## Settings in the palette

Each of these commands changes a setting at once and saves it under `[general]` in
`config.toml`. Going back to the default removes the key. A save that fails shows the message in
the last column and keeps the choice for this session.

| Command | What it does | Says | Saves | If the save fails |
|---|---|---|---|---|
| `:hints` | Turns the key hints off, or back on. | `hints: off` (turning them on: no notice; the hints are the confirmation) | `hints = false` | `Could not save the hints setting: …` |
| `:clock` | Hides the header's clock, or shows it again. | `clock: off` / `clock: on` | `clock = false` | `Could not save the clock setting: …` |
| `:time` | Switches every time of day on screen between 24-hour and 12-hour (see below). | | `time = "12h"` | `Could not save the time setting: …` |
| `:thread-order` | Turns the reader's older messages round (newest first, or oldest first again), in the open thread too. | `thread order: newest` / `thread order: oldest` | `thread_order = "newest"` | `Could not save the thread order: …` |
| `:dim-read` | Cycles how read threads look in the list (see below). | `dim read: dot` / `dim read: subject` / `dim read: all` | `dim_read` | `Could not save the read dimming setting: …` |
| `:field-keys` | Cycles how text fields edit: bash's keys, vi's, or the arrow keys only ([Keys](keys.md#editing-keys-in-text-fields)). | `field keys: vi` / `field keys: basic` / `field keys: readline` | `field_keys` | `Could not save the field keys: …` |
| `search-counts` | Hides the unread count beside each saved search, or shows it again. | `saved-search counts: off` / `saved-search counts: on` | `saved_search_counts = false` | `Could not save the saved-search counts setting: …` |
| `rich-html` | Shows HTML mail as plain text, or with its styles again, in the open reader and every one opened after. | `HTML mail: plain text` / `HTML mail: with its styles` | `rich_html = false` | `Could not save the HTML mail setting: …` |
| `images` | Hides the pictures in HTML mail, or shows them again, in the open reader and every one opened after. | `images: off` / `images: on` | `images = false` | `Could not save the images setting: …` |
| `remote-images` | Loads remote pictures in every thread from now on, the open one too, or holds them back again until `i`. | `remote images: on` / `remote images: off` | `remote_images = true` | `Could not save the remote images setting: …` |

The palette also holds `:theme`, `:accent` ([Themes](#themes)) and `:keymap`
([Keys](keys.md#key-presets)).

### `:time`

`:time` switches every time of day on screen: the list, `● Synced 14:47`, the header's clock, the
Drafts list, `:errors`, the reader's `04:32` and its `Date: Fri 10 Oct 2025 12:40` lines. In
12-hour form they read `2:47 pm`, `4:32 am` and `Date: Fri 10 Oct 2025 12:40 pm`. 24-hour removes
the key.

### `:dim-read`

| Mode | Read threads |
|---|---|
| `dot` (the default) | Keep today's small muted dot. |
| `subject` | Also dim the subject. |
| `all` | Dim the sender, subject, count and time. |

Dimming uses the current theme's muted colour. Unread threads stay bold; the cursor highlight,
account badges, selection marks and action errors keep their colours. Returning to `dot` removes
the key.

## Help

`?` shows the keys of the view you are in, marking with `*` the ones set in `config.toml`. `?`
again (or whatever `global.help` is bound to), `Esc` or `q` closes it.

Help and `:errors` scroll with `j`/`k` or `↓`/`↑`. A `↓ more` or `↑ more` line (`v more` and
`^ more` with `icons = "ascii"`) says there is more beyond the box.

## Sync and connection state

### Each account's state

The status bar shows, on the right, each account's sync state:

| Shown | Meaning |
|---|---|
| `✓` | Caught up |
| `⟳` | Syncing, with a count during the first sync |
| `retrying` | The sync is being tried again |
| `stopped` | The sync has stopped |
| `!` | `needs login` or `sync failed` |

After the state come:

| Shown | Meaning |
|---|---|
| `sending…` | A message is on its way. |
| `saving to Gmail…` (`saving to Microsoft 365…`) | A draft is being saved. |
| `2 queued` | Actions on their way to the account's server. |
| `1 failed` | Actions it refused; see [Doing things to mail](actions.md#how-actions-reach-the-server). |

### The daemon connection

Whether the daemon can be reached shows in the header's summary:

- `connecting to the daemon…`
- `daemon unreachable (Ctrl-r retries)`
- `daemon reachable again (Ctrl-r resumes live updates)`, once a message fetch gets through after
  the connection was lost.

When the daemon cannot be reached the list still reads from the store. `Ctrl-r` reconnects (and
starts a sync) and resumes live updates.

### Store errors

A store read that fails says so where it was shown (the list, the top of the sidebar, or the
reader's subject line) and in the status bar; `:errors` keeps it.

## Themes

Rail paints itself from a colour scheme of its own, on any terminal that shows 24-bit colour
(`COLORTERM` is `truecolor` or `24bit`).

### `:theme`

`:theme` lists the 769 schemes of the
[iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes) collection (MIT),
`Rail default` first.

- Letters filter; `↓`/`↑` and `Ctrl-n`/`Ctrl-p` move.
- The screen takes each scheme as the highlight reaches it.
- `Enter` keeps it and writes `theme = "Name"` under `[general]` in `config.toml` (choosing
  `Rail default` removes the key).
- `Esc` puts the previous look back.

### `:accent`

`:accent` does the same for the accent: the focused pane's border, box borders, titles and
headings, what you type in the palette and the filter, and the sync spinner.

- Choose `scheme default` or one of the scheme's sixteen palette slots, saved as `accent = N`.
- The other panes' borders take the muted colour (palette slot 8 of a scheme) whatever the accent.

The status bar confirms `theme: Dracula` or `accent: 5 magenta`. A save that fails says
`Could not save the theme: …` (`:errors` keeps it) and keeps the look for this session.

### Without truecolor

Without truecolor Rail keeps the terminal's own colours, and the status bar says
`theme needs a truecolor terminal` when a theme is configured and when `:theme` opens.

- `:theme` then does not preview (its hint row says `no preview without truecolor`), and `Enter`
  still saves the choice.
- `:accent` still previews a palette slot, which uses the terminal's own colours there.

Inside tmux, `COLORTERM` must reach the inner shell (see
[Troubleshooting](troubleshooting.md)). An account's own `color` in `config.toml` is kept whatever
the theme.
