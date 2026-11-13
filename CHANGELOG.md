# Changelog

All notable changes to Rail are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- **One pinned Rust version.** `rust-toolchain.toml` now names Rust 1.100.0, not the latest
  stable, and CI, release builds and the development shell all use it, so a new Rust release can
  no longer break every open pull request overnight. Dependabot proposes each new version as a
  pull request that a person merges.

### Fixed

- **CI checks the minimum Rust version again.** `rust-toolchain.toml` outranked the toolchain the
  `msrv` job installed, so the job had been checking with the latest stable Rust instead of the
  minimum. The job is now just `msrv`, so raising the minimum no longer renames a required check.

### Upgrading

- Building Rail from source needs Rust 1.90 or newer (was 1.89). It already did in practice:
  `quantette`, which inline pictures use since 0.10.0, requires 1.90, and the unchecked job
  hid that. The release binaries are unaffected.

## [0.10.3] - 2026-11-11

Trusting your own company's domain now works on Microsoft 365 (#190).

### Fixed

- **Trusting your own company's domain now works on Microsoft 365.** Mail a colleague sends
  inside the organisation carries `dkim=none dmarc=none`, because Exchange signs with DKIM only
  on the way out, so `I` showed the domain as `(not verified for this mail)` and a trusted
  colleague's pictures never loaded on their own. Rail now also accepts Exchange's stamp that
  the sender signed in to the organisation (`X-MS-Exchange-Organization-AuthAs` and
  `X-MS-Exchange-CrossTenant-AuthAs` both `Internal`, each once), for the account's own domain
  only. Exchange strips those headers from mail arriving from outside, so a spoof cannot carry
  them.

### Upgrading

- Internal mail already stored keeps its old result until its body is fetched again, so `I` on
  an older colleague's message may still say `(not verified for this mail)`. New mail is
  checked as it arrives.

## [0.10.2] - 2026-11-11

Sync keeps going when 1Password is locked (#186), the bundled SQLite is patched for
RUSTSEC-2026-0459 (#184), and Dependabot updates merge on their own after a cooldown (#183).
Building from source now needs Rust 1.89.

### Changed

- Dependency updates from Dependabot now wait out a cooldown (3 to 7 days) so a bad release
  can be pulled first, and patch and minor updates merge on their own once every required
  check passes. Major updates still wait for review. See CONTRIBUTING.md.

### Fixed

- **Sync no longer stops when 1Password is locked.** A token refresh that failed (the network,
  or 1Password locked or a prompt unanswered when it saved) threw away the tokens Rail held, so
  every retry had to read them back from 1Password and failed until someone unlocked it; one
  account went five hours without syncing. Rail now keeps its tokens through such failures,
  and a save that fails is logged and tried again at the next refresh.

### Security

- Updated the bundled SQLite to 3.54.1 (`libsqlite3-sys` 0.39.1 through rusqlite 0.41.1) for
  RUSTSEC-2026-0459 (CVE-2026-119375): SQLite before 3.54.0 could return stale process memory
  when it opened a database with a crafted `-wal` file of a different page size. Rail's store
  is in your own data directory, so only someone who can already write there could use it.
- Updated rmcp from 3.8.0 to 3.9.0 for RUSTSEC-2026-0462 (unbounded response buffering in its
  Streamable HTTP client, which only Rail's tests use).

### Upgrading

- Building Rail now needs Rust 1.89 or newer (was 1.88): rusqlite 0.41, which carries the
  SQLite fix, requires it. The release binaries are unaffected.

## [0.10.1] - 2026-11-03

Far fewer 1Password prompts: a token refresh writes back to the secret store only when it must
(#178).

### Fixed

- **Far fewer 1Password prompts.** A token refresh no longer writes to the secret store unless
  it must: a Gmail refresh (the refresh token never changes) writes nothing, and a Microsoft
  365 refresh saves its rotated refresh token at most once a day. The access token stays in
  memory and is refreshed again after a restart. With `secrets = "1password"` this takes the
  daemon from about one prompt an hour per account to one when each account's sync starts. See
  [Tokens in 1Password](docs/configuration.md#tokens-in-1password).

## [0.10.0] - 2026-11-03

Trust a whole domain's remote pictures: `remote_images_from` takes `"@example.com"`, and `I`
asks whether to trust the sender or their domain, offering the domain only for mail its server
vouched for (#176).

### Added

- **Trust a whole domain's remote pictures.** `remote_images_from` takes `"@example.com"` for
  every address at exactly that domain, as well as single addresses. `I` now asks whom to
  trust, the sender or their domain, and offers the domain only when the receiving server
  vouched for the message (DMARC or aligned DKIM), so a forged From line cannot win it. `I`
  then `Enter` trusts the sender as before, and `I` on trusted mail stops trusting whichever
  entry matched. See [Trusting a sender or a domain](docs/reading.md#trusting-a-sender-or-a-domain).

## [0.9.1] - 2026-11-03

Gmail setup for a brand-new Google account: the MCP guide, the setup wizard and the docs now
cover the Cloud terms, choosing the right Google account and browser profile, what Google needs
before it publishes the app, and the one-time client download (#174).

### Changed

- **The Gmail setup guide covers what a new Google account runs into.** `rail_provider_guide`
  (guide version 2) and [Gmail / Google Workspace](docs/accounts.md#gmail--google-workspace)
  now:
  - say to check which account `gcloud` signed in, and how to keep a work account's setup apart;
  - add an optional step to accept the Google Cloud terms, without which a new account cannot
    create a project;
  - open Google's current Auth Platform pages with `authuser`, so a browser signed in to several
    accounts lands on the right one;
  - list what Google now requires before publishing (a homepage and privacy policy URL with
    their domain authorised), and warn that a logo forces Google's verification;
  - say to click Download JSON straight away, as Google shows the client secret only once;
  - tell the agent to offer 1Password for the client secret, and to hand over the login URL for
    the right browser profile.

### Upgrading

- An agent that follows `rail_provider_guide` gets version 2. The Gmail steps gain an optional
  step at index 1, so the steps after it move down one place.

## [0.9.0] - 2026-11-03

Editing keys in text fields: every field takes bash's keys by default, and `field_keys` can
switch them to vi's or to the arrow keys only. `:field-keys` cycles the modes (#152, #153).

### Added

- **Bash's editing keys in text fields.** Every field you type in, from the `/` filter to the
  setup wizard, takes `Ctrl-a`/`Ctrl-e`, `Ctrl-u`/`Ctrl-k`, `Ctrl-w`, `Alt-Backspace`,
  `Ctrl-Delete` and `Ctrl-←`/`Ctrl-→`. `Ctrl-h` removes one character. In a masked field the
  word keys take the whole value as one word. See
  [Editing keys in text fields](docs/keys.md#editing-keys-in-text-fields). Contributed by
  @coko7 (#152, #169).
- **`field_keys`: choose how text fields edit.** `"readline"` (the default) is bash's keys, now
  with `Ctrl-b`/`Ctrl-f` to move a character and `Ctrl-d` to remove one. `"vi"` edits as a
  shell's `set -o vi`: fields open in insert mode, and `Esc` goes to normal mode for motions,
  `d`/`c` operators, `x`, `r` and the rest; a second `Esc` closes the prompt. `"basic"` keeps
  only the arrows, `Home`/`End`, `Backspace` and `Delete`. It is the same in every keymap.
  `:field-keys` cycles and saves it (#153).

### Upgrading

- Keys with `Ctrl` or `Alt` now edit in text fields. Set `field_keys = "basic"` under
  `[general]` to keep the fields as they were.
- An older Rail refuses a `config.toml` that contains `field_keys`; remove it before going back.

## [0.8.0] - 2026-11-02

Key presets: start from the keys you know. `keymap` chooses `rail`, `vim`, `mutt`, `emacs`,
`gmail` or `outlook`; `:keymap` switches and asks whether to keep the new keys, and the setup
wizard offers them on the first run (#153).

### Added

- **Key presets.** `keymap` under `[general]` chooses the keys every action starts from, and
  `[keys]` goes over it. `:keymap` lists the presets and switches at once, then asks **Keep these
  keys?**: `y` saves them, and `n`, `Esc` or 15 s without an answer go back to the keys before.
  A switch that `[keys]` clashes with is refused and the keys stay. The first preset besides
  `rail` is `vim`: `h`/`l` go out and in, `u` undoes, `U` marks read or unread, `Z` snoozes,
  `L` labels, `D` trashes without asking and `Ctrl-f`/`Ctrl-b` page (#153). Prompted by
  @coko7's request for bash's editing keys in text fields (#152).
- **The `mutt` preset** (`keymap = "mutt"`): `s` moves, `F` stars, `N` marks read or unread,
  `t` selects, `l` filters, `g` replies to everyone, `m` writes, `$` syncs and `u` undoes; `d`
  still asks and `D` trashes without asking. Top and bottom are `=` and `*`, the links `Ctrl-o`
  and `Ctrl-b`, select all or by sender `T a` and `T f`; in the reader `h` shows the headers,
  `i` goes back, `v` lists the attachments and `P` loads the pictures. Rail's own keys are
  unchanged (#153).
- **The `emacs` preset** (`keymap = "emacs"`) adds emacs's keys beside Rail's own: `Ctrl-n`/
  `Ctrl-p` move, `Ctrl-v`/`Alt-v` page, `Alt-<`/`Alt->` go to the top and bottom, `Ctrl-s`
  filters, `Ctrl-g` backs out and `Alt-x` opens the palette; `Ctrl-x u` undoes and `Ctrl-x o`
  moves between the sidebar and the list. In the composer `Ctrl-x Ctrl-s` saves to Drafts,
  `Ctrl-x i` attaches and `Ctrl-x k` discards. `n`/`p` move through the list (#153).
- **The `gmail` preset** (`keymap = "gmail"`) follows Gmail's web shortcuts: `o` opens, `u`
  goes back to the list, `I`/`U` mark read and unread, `#` trashes without asking, `v` moves,
  `b` snoozes, `x` selects, `* n` clears the selection, `a` replies to everyone, `N` syncs, `g i`
  goes to the inbox and `Ctrl-s` saves the draft. The sidebar moves to `B`, a range to `V`, the
  attachments to `A` and trusting a sender's pictures to `T` (#153).
- **The `outlook` preset** (`keymap = "outlook"`) follows Outlook for Windows and on the web:
  `Delete` trashes without asking, `Backspace` archives, `Insert` stars, `Ctrl-q`/`Ctrl-u` mark
  read and unread, `Ctrl-z` undoes, `Ctrl-n` writes, `Ctrl-r` replies, `Ctrl-f` forwards, `v`
  moves, `Ctrl-e` filters, `Ctrl-a` selects all and `F9` syncs; `Alt-s` sends and `Ctrl-s` saves
  a draft. A range moves to `V` (#153).
- The setup wizard's first run ends with **Choose your keys**: Rail's own keys or any preset,
  saved as `keymap` (#153). A preset that the config's `[keys]` clash with is refused, and the
  wizard still finishes, saying `:keymap` can choose the keys later.
- `F2` opens the palette in every preset, whatever `[keys]` says, so switching presets never
  hides the way back.
- `list.trash_now`: move to the trash without asking, still undoable and still asked about for a
  large selection (`bulk_confirm`) and in Drafts. It has no key in the `rail` preset: `D` in `vim`
  and `mutt`, `#` in `gmail`, `Delete` in `outlook`.
- `list.mark_read` and `list.mark_unread`: mark read, or mark unread, whatever the threads show,
  where `u` toggles. Neither has a key in the `rail` preset: `I`/`U` in
  `gmail`, `Ctrl-q`/`Ctrl-u` in `outlook` (#153).
- `delete`, `backspace` and `insert` are key names for `[keys]`, shown as `Del`, `Bksp` and
  `Ins` in help. No default key uses them.

### Changed

- The README is now a short overview, and the full documentation lives in `docs/`: account
  setup, reading, acting on and writing mail, keys, configuration, commands, MCP, security and
  troubleshooting, with `docs/README.md` as the index. Release archives carry `docs/` too.
- Messages that pointed to the README now name the page: the Microsoft 365 consent error says
  `see "Work or school accounts and admin consent" in docs/accounts.md`, and an unknown `[keys]`
  action says `(docs/keys.md lists them)`.
- Updated `async-executor` to 1.14.1: 1.14.0, which `keyring`'s Linux Secret Service client
  pulls in, was yanked.

### Fixed

- Keys reach the mail screen when the terminal never answers Rail's picture query, as in a tmux
  session with no client attached or a pseudo-terminal nothing reads. The unanswered query left
  a reader behind that took every key and later turned raw mode off, so keys echoed and the
  screen ignored them. Rail now asks only a terminal that answers, and only a tmux session with
  a client attached; any other gets half-block pictures (#155).

### Upgrading

- A `[keys]` entry that binds `F2` to another action now stops Rail from starting; move that
  action to another key.
- An older Rail refuses a `config.toml` that contains `keymap`; remove it before going back.
  It also refuses `[keys]` entries naming `delete`, `backspace` or `insert`.

## [0.7.0] - 2026-11-02

Security hardening of remote pictures: a trusted sender's pictures load only for mail the
receiving server vouched for, the daemon bounds its picture fetches and cache, and IPv6 can be
turned off for them (#131, #132, #134).

### Added

- `remote_images_ipv6` under `[general]` (default `true`). `false` makes the daemon fetch remote
  pictures over IPv4 only. On an IPv6-only network with NAT64, the gateway's own prefix looks
  public to Rail's address guard but can be translated to private IPv4 addresses; the README now
  says so (#134).

### Fixed

- The daemon fetches at most eight remote pictures at once, however many readers ask, and the
  image cache (`cache/images/`) holds at most 512 MiB: the oldest pictures go to make room for a
  new one, besides the weekly sweep. A mailbox of picture-heavy mail can no longer fill the disk
  before the sweep (#132).

### Security

- A trusted sender's remote pictures (`remote_images_from`, `I`) load by themselves only when
  the receiving server vouched for the sender's domain: the topmost `Authentication-Results`
  (Gmail's own, from `mx.google.com`, or Exchange Online's) shows DMARC passed for the From
  domain, or a DKIM signature of it or a parent domain passed. Mail with a forged From line now
  waits with the hint, as an untrusted sender's does, so its pictures no longer signal that you
  read it. `I` still loads the open thread's pictures at once (#131).

### Upgrading

- This version adds a database migration (schema v12: `messages.vouched_domains`). An older
  Rail refuses the upgraded store.
- Mail stored before this version has no vouched domains until its body is fetched again, so a
  trusted sender's older mail shows the hint; `i` loads it.
- An older Rail refuses a `config.toml` that contains `remote_images_ipv6`; remove it before
  going back.

## [0.6.0] - 2026-11-01

### Added

- **Link actions.** Mail from an issue tracker carries the record's id in its subject, so
  `[[link_actions]]` turns that id into the record's address and `g t` (`list.open_link`,
  in the list and the reader) opens it:

  ```toml
  [[link_actions]]
  name  = "Ticket"
  match = '\(#(\d+)\)'
  url   = "https://tracker.example.com/ticket/{1}"
  ```

  `{1}`, `{2}`, … are the pattern's capture groups. Every match of every rule is offered, so
  a subject carrying two ids reaches either; one match opens after the same confirmation as
  any other link, several are listed by number with the rule that found each and the address
  it opens (a narrow terminal cuts an address's middle, so its id stays visible), and one
  address is one choice however many matches produced it. Suggested and contributed by
  @mheyd (#138, #142).

### Security

- A link action's captures are percent-encoded before they reach its template, and a capture
  of `.` or `..` drops the match rather than climbing out of the path the rule chose: a
  subject is the sender's text. A template whose own path holds such a segment, written with
  dots or `%2e`, is refused when the config loads, because an empty capture beside it climbs just as far.
- The destination host comes from the config alone. A template whose scheme, credentials,
  host or port a match would decide is refused when the config loads, which Rail settles by
  filling the template twice and reading both with the parser that opens the link, so the
  check cannot disagree with what the address turns out to mean.
- `mail-builder` 2.0 drops ASCII control characters in an address when writing it, so a CR LF
  there could not inject header fields. Rail already refused these itself.

### Changed

- Updated dependencies: `mail-builder` 2.0, `rmcp` 3.8, `unicode-width` 0.2.3 and `rustix` 1.1.6.

## [0.5.1] - 2026-10-30

### Fixed

- `I` in the reader adds or removes only that sender in `remote_images_from` as `config.toml` has
  it, so a sender added by another Rail window or by hand is no longer lost. The list's other
  entries, their formatting and a trailing comment stay. A running Rail still picks up senders
  added elsewhere only when it restarts. Contributed by David Hallas (@dhallas) (#133, #136).

## [0.5.0] - 2026-10-30

Remote pictures in HTML mail, loaded only when you ask, contributed by David Hallas (@dhallas)
(#129).

### Added

- **Remote pictures.** The pictures an HTML message loads from the web are held back, because
  fetching one tells its sender you opened the mail. A message that has them starts with
  `Remote pictures are not shown; i loads them, I always for this sender.`
  - `i` loads the open thread's remote pictures.
  - `I` always loads them for the sender of the message under the cursor, saved as
    `remote_images_from` under `[general]`, and `I` again stops that.
  - `remote_images = true` under `[general]`, or `:remote-images`, loads them in every thread.
- The daemon fetches remote pictures itself, never the terminal:
  - only over `https`, from public addresses, with every redirect checked the same way;
  - with no proxy, cookies or referrer;
  - at most 5 MiB each, with the type read from the bytes;
  - kept for a week in Rail's private image cache.

### Security

- Trusting a sender trusts any mail whose From line carries that address, and a From line can be
  forged. The README says so; checking the provider's authentication results is #131.

### Upgrading

- An older Rail refuses a `config.toml` that contains `remote_images` or `remote_images_from`;
  remove them before going back. No database migration: going back to 0.4.x is otherwise
  supported.

## [0.4.1] - 2026-10-29

### Fixed

- Inline pictures of mail stored before 0.4.0 show the first time the message is opened: Rail
  fetches that message once more to learn its pictures' Content-IDs, and that message's attachment
  details are corrected then, including ones Rail kept after Microsoft 365 refused its attachment
  list. Messages without inline pictures are not fetched again, so their attachment details stay
  as stored (#125).

## [0.4.0] - 2026-10-29

HTML mail with its styles and inline pictures, and Microsoft 365 folders that have subfolders,
contributed by David Hallas (@dhallas) (#112, #113, #114, #115, #116).

### Added

- **Styled HTML mail.** The reader draws HTML mail's bold, headings, italics, quotes and links in
  those styles. The text is still the text Rail stores and searches, so link markers, `g x`,
  hyperlinks, search and quote folding work as before. `rich_html = false` under `[general]` or
  `:rich-html` shows the plain text instead.
- **Inline pictures.** The reader draws the pictures an HTML message carries itself (`cid:`), where
  the message puts them and at the size it asks for. It uses the terminal's image protocol (kitty,
  iTerm2 or sixel), or coloured half blocks elsewhere. `[image: …]` stands in until a picture
  arrives, and for any past the first 32 in an open thread. Tracking pixels are left out, and
  remote pictures are never fetched. `images = false` under `[general]` or `:images` leaves them
  out.
- Rail keeps each attachment's Content-ID, and fetches a picture it draws into a private cache
  (`cache/images/`, swept after a week). Only PNG, JPEG, GIF and WebP up to 5 MiB are drawn, and
  only when the bytes match the declared type.

### Changed

- On Microsoft 365, `Enter` on a More folders row named after a system folder (such as `Inbox`
  above `Inbox/Project`) opens that folder instead of unfolding the row. Space, ← and → still fold
  it. Gmail label paths such as `Sent/2024` unfold as before (#112).
- Rail asks the terminal once at start how it draws pictures, which can take up to 2 s on a
  terminal that does not answer. It does not ask when `TERM` is unset, empty or `dumb`.

### Fixed

Found in review before release and fixed on the contributor's branches:

- The command palette lists a system folder once when a Microsoft 365 subfolder sits under it
  (#117).
- `Enter` on a Gmail label path named like a system label unfolds it instead of opening the system
  mailbox (#118).
- The reader's cursor stays on its line when a message's styled view arrives (#119).
- The styled view shows the note that Rail kept only the first 10 MiB of a long message (#120).
- When Microsoft 365 refuses to list a message's attachments, Rail keeps the message's text and the
  attachments it already had. Importing an Outlook draft in that state stops with an error instead
  of dropping its attachments (#121).
- A cached picture is no longer swept just after Rail hands it to the reader (#122).
- One message with many large pictures can no longer exhaust the reader's memory (#123).
- Start no longer stalls on a terminal that cannot answer the picture query (#124).

### Upgrading

- This version adds a database migration (Content-IDs). An older Rail refuses the upgraded store,
  so going back to 0.3.x is not supported.
- Mail stored before this version gains Content-IDs, and so its inline pictures, only when its body
  is fetched again.
- An older Rail also refuses a `config.toml` that contains `rich_html` or `images`; remove them
  before going back.

## [0.3.0] - 2026-10-27

Bulk selection, snooze, attachments and saved searches, contributed by @armangurkan (#100, #101,
#102, #103; proposals #81 and #98).

### Added

- **Bulk selection.** `* a` selects every thread the mailbox and filter match (up to 10,000),
  `* f` every thread from the sender of the thread under the cursor, `v` a range, and `J`/`K` or
  Shift-↓/Shift-↑ extend the selection one thread at a time. Archive, trash, star, read, labels
  and move act on large sets in chunks of 200 threads per account, and `z` undoes the whole set.
  Over 50 threads `e`, `s` and `u` ask first (`[general] bulk_confirm`, `0` never asks); trash
  always asks.
- **Snooze.** `h` archives the thread, the selection or the open thread now and puts it back in
  the inbox, unread, at a chosen or typed time (`6pm`, `tomorrow 9am`, `3 days`, `next fri`), or
  earlier when new mail arrives in it. A Snoozed mailbox lists what is waiting; "Return to inbox
  now" brings a thread back early. Rail keeps the times itself, since neither Gmail nor
  Microsoft 365 offers a snooze, and returns threads while the daemon runs.
- **Attachments.** `a` in the reader lists a thread's attachments. `s`, `S` and `w` save them to
  `[general] attachment_dir` (your download directory by default) as owner-only files that never
  replace an existing file. `Enter` or `o` opens common document and image types with the
  system's program after a once-a-session question; anything else is saved instead.
- **Saved searches.** `S` (or `:save-search`) keeps the filter in force under a name in
  `[[saved_searches]]` in `config.toml`. Each search is a sidebar row under SEARCHES with an
  unread count (`:search-counts` / `saved_search_counts` turns counts off), listing every folder
  of every account narrowed by its filter. `r` and `d` rename and remove it.

### Changed

- The reader's headers key moved from `h` to `H`; `h` is snooze. A `[keys]` entry binding
  `reader.headers` to `h` now stops Rail at startup with a clash error until you change it.
- The new default keys `J`, `K`, `shift-down` and `shift-up` count towards clashes: a `[keys]`
  entry binding one of them to another list action stops startup until you change it.

### Security

- Rail opens an attachment only when its bytes match the type it declares, and refuses text
  that contains markup a system opener could sniff as HTML or SVG; such an attachment is saved
  instead (#107).
- The open question warns that Office documents and RTF can hold macros or links that run or
  fetch content.
- An opened attachment's copy lives in a private folder of Rail's cache and is removed a minute
  after the reader closes, when Rail quits, or by a daily sweep (#108).

### Fixed

Found in review before release and fixed on the contributor's branches:

- A late `* a` or `* f` answer is no longer applied after you switched mailbox or filter (#104).
- `* f` no longer selects by your own address when you sent the last reply (#105).
- A snoozed thread whose return to the inbox is refused is no longer lost: the failure stays in
  `:errors` and `Enter` sends it again, without bringing back mail you trashed meanwhile (#106).
- Renaming a saved search keeps a query you edited in `config.toml` while Rail was open (#109).
- A saved search with an empty query, or one that narrows nothing, lists nothing instead of
  every thread, and is refused when saved (#110).

### Upgrading

- This version adds a database migration (snoozes). An older Rail refuses the upgraded store,
  so going back to 0.2.x is not supported.
- An older Rail also refuses a `config.toml` that contains `[[saved_searches]]`,
  `saved_search_counts`, `bulk_confirm` or `attachment_dir`; remove them before going back.

## [0.2.4] - 2026-10-25

### Added

- Release binaries for Linux ARM64 and for macOS on Apple Silicon and Intel, built by GitHub
  Actions from the release tag with build provenance attestations.

### Fixed

- `sync.status` now counts a refused change as failed by the time `action.failed` arrives, so
  a client that checks the status on the notice no longer sees the old count.
- A config save from the TUI (`:theme`, `:time`, `:hints`, ...) and a daemon edit of the
  config (adding or removing an account, changing where tokens are stored) at the same moment
  could lose one of the two changes. Every edit of `config.toml` now holds a lock on
  `config.lock` beside it, and each write uses its own temporary file. Reported by
  @armangurkan.

## [0.2.3] - 2026-10-25

### Added

- Page the sidebar a screen at a time with Ctrl-d and Ctrl-u (also PageDown and PageUp).
  Thanks to @dhallas.

### Security

- Updated rustls-webpki to 0.103.16 (RUSTSEC-2026-0382) and webpki-roots to 1.0.11
  (RUSTSEC-2026-0381), which affect certificate verification for the Gmail and Microsoft
  Graph connections.

### Changed

- Updated Tokio from 1.53.3 to 1.54.1 and rmcp from 3.5.1 to 3.7.0.
- Updated the CI cache, tool installer and CodeQL actions, and grouped the CodeQL action
  updates so both steps always move together.

## [0.2.2] - 2026-10-18

### Added

- Linux x86_64 release binary with SHA-256 checksums (glibc 2.36 or newer).

### Changed

- Updated Tokio from 1.53.2 to 1.53.3, including runtime and synchronization fixes.
- Updated the pinned Rust toolchain action used by CI.
- Documented exceptions for three maintenance advisories in the Linux keyring
  dependency tree while retaining Rust 1.88 support and vulnerability checks.

## [0.2.1] - 2026-10-15

### Changed

- Updated `serde_json`, `schemars`, `sha2` and `signal-hook` dependencies.
- Added project status badges and automatic publication of measured code coverage from CI.

## [0.2.0] - 2026-10-15

### Added

- `dim_read` under `[general]` selects `"dot"` (the current look and default), `"subject"`
  (also dim read subjects), or `"all"` (dim read sender, subject, count and time). `:dim-read`
  cycles the modes immediately and saves the choice. The theme supplies the muted colour;
  cursor highlighting, account colours and action errors stay readable.

- `clock = false` under `[general]` hides the header's clock; `:clock` in the palette hides it
  or shows it again for the session (`clock: off`, `clock: on`) and saves the choice. A save
  that fails says `Could not save the clock setting: …` and keeps the choice.
- The header's right edge has the time of day after the sync summary
  (`● Synced 14:47 │ 15:02`, `3:02 pm` with `time = "12h"`), and a search region before the
  slot (`│ 🔍 / Search`, `| / Search` with `icons = "ascii"`). Under 60 columns both go. At
  `split_threshold` and wider, and at least 12 rows high, the header is boxed like the panes
  (`│ Rail │ All inboxes …`, without the `▍` bar).
- `:thread-order` in the palette turns the reader's older messages round for the session
  (`thread order: newest`, `thread order: oldest`), in the open thread too, and saves the
  choice as `thread_order` under `[general]`; a save that fails says
  `Could not save the thread order: …` and keeps the choice.
- `time = "12h"` under `[general]` writes every time of day on screen with am/pm (`2:05 pm`,
  `Mon 6:20 pm`, `Fri 10 Oct 2025 12:40 pm`, `● Synced 2:47 pm`); `"24h"` is the default.
  `:time` in the palette switches for the session and saves the choice.
- `label` in an account's `[[accounts]]` table names the account on screen: the sidebar, the
  header (`Personal · Inbox`), `Write from`, the account states and the palette; `name` stays
  what commands, `--account` and `account:` use. At most 40 characters; `rail doctor` warns
  `two accounts share the label "X"`.
- Starred, Drafts, Sent and Trash of every account together (`Mailbox: Starred` and so on in
  the palette): newest first, with the filter and the keys working as in the unified inbox and
  each action going to the thread's own account. The header says
  `Starred · 2 accounts · 12 unread`, `Sent · 2 accounts`; Sent and Trash match each provider's
  own folder (`Sent Items`, `Deleted Items` on Microsoft 365).
- The sidebar's look: `UNIFIED` and `ACCOUNTS` headings, rules under UNIFIED and above each
  `More folders`, `▾`/`▸` fold arrows, a badge per account (its label's first letter on its
  colour), an icon per mailbox (`✉ ★ ✎ ➤ ✕ ▣ ⊘ 📁`; letters with `icons = "ascii"`), counts
  right-aligned with thousands separators, and the current mailbox's row in the accent colour
  while the sidebar does not have focus. The sidebar scrolls to keep that row, or the cursor, in view.
- The mail screen's frame: a header row with the scope, its counts, a search slot and one sync
  summary (`● Synced 14:47`, or the worst account's state; kept whole, the scope is cut when both
  do not fit); the sidebar, the list and the reader
  boxed at `split_threshold` (140 columns by default) and wider, the focused pane in the accent
  colour (`+`, `-` and `|` with `icons = "ascii"`); and a status bar of key hints for the pane
  you are in, built from your keymap, with each account's state on the right, each key in its
  own drawn box at `split_threshold` and wider.
  Notices (`archived 3 · z undoes`) show in place of the hints. `3 selected` and the daemon's
  state moved from the status bar to the header.
- `hints = false` under `[general]`, and `:hints` in the palette, turn the key hints off and give their rows back to the panes.
- Writing Microsoft 365 mail over Microsoft Graph: `c`, `r`, `R` and `f` write from a Microsoft
  365 account and `y` sends with the ten second `z` undo; `s` saves to Outlook's Drafts
  (`saved to Microsoft 365 · z undoes`) and later saves update the same draft, or make it again
  in Drafts when it was deleted in Outlook; Outlook drafts open in Rail with their attached files
  and `d` deletes them in Outlook. Replies and forwards are made on
  the original message, so they join the Outlook conversation; one whose original is gone from
  Outlook for good goes out as a new message. Attachments up to 25 MiB in all, through upload
  sessions from 3 MiB, each uploaded once until it changes; a forward keeps the original's
  attachments without downloading them. Rail writes plain text, so pictures inside the body are
  dropped, while attached messages and OneDrive or SharePoint links stay as Outlook has them. A
  message is never sent twice: a retry or a restart asks Outlook for its `Message-ID`, and about
  the draft Rail saved, first.
- Store schema v8: a draft keeps the provider id of the message a reply or forward answers, and
  an attachment says whether it is a plain file (Microsoft 365 attachments are listed again).
- Microsoft 365 thread actions over Microsoft Graph: `e` archives to Outlook's Archive folder,
  `d` moves to Deleted Items, `m` moves to Inbox, Archive, Junk Email or one of the account's
  folders, `s` flags the newest message or clears every flag, and `u` marks every unread message
  read or the newest one unread, each with the ten second `z` undo. Archive, trash and move leave
  Sent Items and Drafts where they are. `l` answers `Outlook uses folders, not labels: press m to
  move.`, and archive in a mailbox with no Archive folder answers `This mailbox has no Archive
  folder; press m to move.` The trash prompt names the provider's trash.
- Microsoft 365 sync and reading over Microsoft Graph: every mail folder (nested ones as
  `Parent/Child`, the well-known ones in their usual places, search and non-mail folders left
  out), threads by conversation, read and flag state, one combined delta cursor per account, new
  mail within one poll interval, `$search` server search, bodies through the shared HTML
  renderer, attachment lists, and marking read on open reaching Outlook. Labels (`l`)
  on a Microsoft 365 account are refused at once.
- The HTTP layer speaks Microsoft Graph: immutable ids on every request, Graph's error envelope,
  at most four requests in flight per mailbox, and `$batch` with throttled items sent again.
- Writing mail: `c` new message, `r` reply, `R` reply-all, `f` forward, written in your editor
  (`general.editor`, `$VISUAL`, `$EDITOR`, `vi`) and reviewed in Rail; `To:` completion from the
  addresses your mail has taught the store; attachments by path and forwarded attachments; send
  through the outbox with the 10 second `z` undo, never sent twice; local drafts kept on every
  editor round and reopened with `:drafts` or from the Drafts folder; `from_name` and `signature` per account.
- Drafts on Gmail: `s` on the review saves the draft to Gmail's Drafts after the ten second undo
  (`z` cancels it), creating the Gmail draft once and updating it after; Rail's version wins over
  an edit made elsewhere. A draft that is sent has its Gmail copy deleted; a failed delete is
  listed in `:errors`. A Gmail draft written elsewhere opens at its review, with its attachments
  fetched once into private files the daemon owns (`draft-files` in the data directory, removed
  when the draft is discarded and swept at daemon start).
- The Drafts folder: each account's `Drafts` in the sidebar lists local and Gmail drafts in one
  pane (`Enter` resumes or imports, `d` discards); `:drafts` lists the same rows and merges every
  account's in the unified inbox. Thread actions there say `Not available on drafts`.
- `draft.push` and `draft.import` operations (terminal only; the MCP tool list is unchanged).
  `OutboxDepth` gains `saving`, the drafts being saved to Gmail, shown as `saving to Gmail…`.
- Store schema v7: `drafts`, a stored `Reply-To`, outbox rows keyed by draft.
- `draft.save`, `draft.send` and `draft.discard` operations (terminal only); `action.undo` and
  `action.retry` now take the client token of the row they act on.
- Two-crate Cargo workspace (`rail-core`, `rail`) on Rust 2024 with `unsafe_code = "forbid"`,
  clippy as errors, `cargo deny`, and CI for Linux and macOS.
- XDG paths with `RAIL_CONFIG_DIR`, `RAIL_DATA_DIR`, `RAIL_CACHE_DIR` and `RAIL_STATE_DIR`
  overrides.
- `config.toml` loading and validation with file path and line number in every error, plus
  comment-preserving edits.
- SQLite store with embedded migrations and the full v1 schema, including FTS5 search tables.
- Secret storage in the OS keyring, with a 0600 file fallback (`secrets = "file"`).
- OAuth2 authorization-code login with PKCE over a hardened loopback listener for Gmail and
  Microsoft 365.
- Cached, single-flight token refresh that keeps the old refresh token when the provider omits it.
- Commands: `rail account add`, `rail account list`, `rail doctor`.
- File logging to the state directory.
- Setup wizard: a full-screen ratatui wizard that adds an account from an organisation preset or
  from your own app registration, shows the portal steps, validates as you type, logs in through
  the browser and reports where everything was saved. It opens on `rail` with no config, on
  `rail setup`, and on `rail account add` without flags (or with only a provider, which it
  preselects). It exits 0 when an account was saved and 1 ("setup cancelled; nothing was saved")
  otherwise, and holds `q`, `Esc` and `Ctrl-C` while an account is being saved.
- Organisation presets: TOML files discovered in `~/.config/rail/presets/` and
  `/etc/rail/presets/`, or fetched from an `https://` URL (64 KiB cap, plain `http` refused).
- Microsoft 365 admin consent from inside the wizard, and automatic routing when a login reports
  `AADSTS65001` or `AADSTS90094`: administrators go to the consent step, everyone else gets the
  admin-consent URL to send to one.
- `rail account login <name-or-address>` to log in again and replace an account's tokens.
- The app shell the mail views will run in: terminal guard and panic hook that always restore the
  terminal, an async event loop with cancellable background tasks, and reusable widgets.
- A background daemon (`rail daemon run|status|stop`) that owns the store, the token stores and
  every login, started on demand and stopped after ten minutes idle. It listens on a Unix socket
  in a 0700 runtime directory (`$XDG_RUNTIME_DIR/rail`, or `RAIL_RUNTIME_DIR`), mode 0600, and
  drops connections from other users. Clients started together share one daemon, which keeps the
  environment and the `RAIL_*` directories of the client that started it and refuses clients whose
  directories differ. At most eight logins wait at once; SIGINT and SIGTERM let saves in flight
  finish. A client replaces only an older daemon; a client older than the running daemon (an open
  `rail mcp` after an upgrade) says "Rail was upgraded; restart this program" instead of stopping it.
- The operation layer: every user operation is a typed request and response (JSON-RPC 2.0, one
  message per line, 1 MiB cap) shared by the TUI, the flag commands and the MCP server.
- `rail mcp`, a stdio MCP server for AI coding agents (`claude mcp add rail -- rail mcp`) with
  twelve tools, a `setup` prompt and guided Google (`gcloud`) and Microsoft (`az`, work or school
  accounts) app registration. `rail_account_add` takes the absolute path of a client secret file,
  an `op://` reference or a preset, never a raw secret; tools refuse arguments they do not know.
  `rail_account_remove` and `rail_secrets_configure` change things only after the user confirms
  through MCP elicitation; without it nothing changes.
- `rail secrets set <keyring|file|1password> [--vault V] [--account A] [--no-migrate]`, the
  terminal's way to move tokens between backends (as `rail_secrets_configure` is the agent's):
  interactive only, it shows the daemon's description of the change and asks `Change? [y/N]`.
- `rail account add --store-secret-in-1password` stores the given client secret in 1Password and
  writes an `op://` reference, as `store_secret_in_1password` on `rail_account_add` does.
- `rail account remove <name-or-address>`, interactive only: it shows what goes and asks
  `Remove? [y/N]`, has no flag to skip the question, and removes nothing when stdin or stdout is
  not a terminal. The account's mail stays in the store.
- The 1Password token backend (`secrets = "1password"`, `[onepassword]`), `op://` references for
  client secrets (also in the wizard and `--client-secret`), and moving tokens between backends
  with `rail_secrets_configure`.
- The per-account `ai_send` policy (`confirm`, `drafts`, `client`), recorded and reported by
  `rail doctor` and `rail_status`; enforcement arrives with the send tools.
- Gmail sync in the daemon: labels, envelopes for `history_days` (the whole mailbox by default)
  and bodies for threads newer than `body_cache_days`, kept current every `poll_interval` by an
  always-on daemon while accounts are configured. Backfill runs newest first, so the inbox is
  usable within seconds, and resumes after a crash; an expired history cursor triggers a full
  resync, a revoked login pauses only that account, and rate limits are paced and retried. A first
  sync of a large mailbox takes hours (about 250 messages a minute, from Gmail's quota). HTML
  bodies become plain text through Rail's own linear scanner without loading anything remote;
  tables, list numbers and link addresses are not rendered, and display-quality rendering is a
  later-phase item.
- Per-account `history_days` and `body_cache_days` overrides.
- `rail sync [<account>] [--no-wait]` with progress, followed with no overall limit (it stops
  watching after 10 minutes without change, and the sync continues in the daemon), and exit codes
  0 (caught up), 1 (failed), 2 (needs a login; 2 wins over 1).
- `rail search <query> [--account <name>] [--limit N]`: the local store and Gmail's whole mailbox,
  merged by date, one escaped line per thread; results on standard output, notes on standard error.
- `rail daemon install` / `rail daemon uninstall`: start the daemon at login (systemd user unit or
  launchd agent). The unit sets the `RAIL_*` directories and a fixed `PATH`, unsets display,
  browser and 1Password variables, refuses binary paths systemd cannot run, checks the unit's daemon
  starts, and is rolled back if enabling fails. The launchd path has not been tried on a real Mac.
- `rail doctor` shows each account's sync and whether the login unit is installed, enabled and
  current, and fails for a Gmail account with no tokens or whose sync stopped on a permanent error.
- MCP tools `rail_sync_status` and `rail_sync_now`, which return no mail content.
- An ignored, read-only live test against a real Gmail account
  (`RAIL_GMAIL_LIVE_ACCOUNT=<name> cargo test -p rail-core -- --ignored gmail_live`).
- The mail screen: `rail` with an account configured opens a thread list over the local store
  (the unified inbox, one account's inbox with `1`-`9`, any label from the sidebar), keyset paged
  so the bottom of a label with thousands of threads is a keypress away, kept current from the
  daemon's progress notifications, with ASCII glyphs under `general.icons = "ascii"`.
- The reader: messages oldest first with unread ones and the latest expanded, bodies wrapped to
  the pane, quoted runs folded to `[n quoted lines]`, numbered link markers (OSC 8 hyperlinks
  where the terminal supports them) and `g x` to open one after showing its address, a read-only
  attachment list (`a`), and the pane-or-full-screen layout by `general.split_threshold`.
- `o` opens a message's original HTML in the browser from a private temporary file, after a
  once-per-session confirmation that remote images will load there.
- Filter as you type (`/`) with `from:`, `to:`, `subject:`, `has:attachment`, `is:unread`,
  `is:starred`, `before:`, `after:` and `account:`; the command palette (`:`) with fuzzy matching
  over commands, accounts and labels; `?` help generated from the keymap; `:errors`.
- `[keys]` in `config.toml`: every key is a named action that can be rebound; bad entries are
  refused with the file and line.
- `body.fetch`, a daemon operation that fetches bodies the store does not hold yet; the reader
  shows "fetching…" meanwhile. It answers outcomes, never bodies, and is not an MCP tool.
- Link footnotes in stored bodies (`messages.links`, store version 4).
- Actions on Gmail threads from the thread list and the reader: `e` archive, `d` trash (asks
  first), `s` star, `u` read or unread, `l` labels, `m` move to a label, alone or on a selection
  (`Space`, `Esc` clears). Each applies to the store at once, can be undone with `z` for ten
  seconds, and is sent to Gmail by the daemon's per-account outbox worker, surviving restarts.
  Opening a thread marks it read. The status bar shows queued and failed actions; an action Gmail
  refuses is undone locally, shown on the thread's row and kept in `:errors`, where `Enter`
  retries it.
- Store schema v6: the outbox's scheduling columns (`not_before`, `thread_ids`, `inflight_at`,
  `reversal`).
- `action.apply`, `action.undo` and `action.retry` operations (terminal only; no MCP tool can
  change mail), the `action.failed` notification, and `outbox` counts in `sync.status`.
- Themes: `:theme` picks one of the 769 iTerm2-Color-Schemes schemes with a live preview and
  saves it as `general.theme`; `:accent` picks the accent slot (`general.accent`). Rail paints
  its own background when a theme is chosen, on truecolor terminals, and keeps the default
  look elsewhere. The free-form `[theme]` table placeholder is gone.

### Changed

- The reader opens on a title block: the subject, the newest message's sender, its time as the
  list writes it (yesterday with its weekday, `Thu 08:55`) and the account's badge and label,
  its recipients by name with the thread's size (`To: Vito  /  4 messages`), and a rule; the newest message is open under it, and the
  subject scrolls away with it. The older messages are cards between rules, oldest first
  (`thread_order = "newest"` under `[general]` runs them newest first): each shows its number in
  the thread, its sender (bold while unread) and its time at the right edge over a line of its
  text, and `Tab` or `Enter` opens it in place. Unread older messages no longer open on their
  own; `n`/`p` jump between the newest message and each card. `h` shows or hides every open
  message's `From:`, `To:`, `Cc:`, `Date:` and `Subject:` lines, hidden at first.
- The thread list is in date groups (`TODAY`, `YESTERDAY`, `THIS WEEK`, `OLDER`), the group of
  the top row kept on the first row. A row reads dot, badge, people, `★`/`📎`, subject `(n)`,
  time: the time at the right edge (`14:05`, `Mon 18:20`, `Oct 03`, `2024-10-03`) replaces the
  date column, the message count follows the subject and is never cut, a read thread's dot is a
  muted `·` (`.` with `icons = "ascii"`), and in `All inboxes`, `Starred`, `Sent` and `Trash`
  each row carries its account's badge. Mail dated after today is under `TODAY` with its date.
  A page moves by the threads that fit.
- The sidebar has two sections. UNIFIED: `All inboxes`, `Starred`, `Drafts`, `Sent` and `Trash`
  across accounts. ACCOUNTS: each account with only its system folders; every other folder or
  label is under `More folders (N)` as a tree of its path, Gmail's categories under
  `Categories`. `Space`, `←` or `→` folds an account, `More folders` or a path, for the session;
  `Enter` on `More folders` or on a path with no folder of its own unfolds it. Counts are unread
  threads, drafts on `Drafts`, none on `Sent` and `Trash`. The palette lists labels by their
  whole path (`Label: AXON · Inbox/Infrastructure/Linode`).
- The filter is typed in the header's search slot, and the compose review's key row is the last
  row's hints, so the review has one more row for its content. While a `t` or `a` prompt is open
  the row shows `Enter accept  Tab complete  Esc cancel`.
- What writing says names the account's provider: `saved to Microsoft 365 · z undoes`,
  `Its copy in Outlook's Drafts is deleted too.`, `Microsoft 365 no longer has this draft`,
  `draft: saving to Microsoft 365, try again in a moment`, `s save to Microsoft 365` on the review
  and the 25 MiB refusal; Gmail's wording is unchanged. The provider trait writes one
  `OutgoingMail` (`send`, `draft_save`, `sent_already`), and Gmail's MIME is built inside its
  adapter, with the same requests on the wire.
- `Provider::apply(threads, change)` replaces `modify_threads` and `trash_threads`: a semantic
  `Change` (archive, trash, star, read, labels, move) that each adapter turns into its own calls;
  Gmail's wire calls are unchanged. `ThreadTarget`s carry each stored message's folder, flags and
  date, as they were before the action's own local change. `Profile.messages_total` is optional;
  `RemoteFolder` has a `parent`, stored in `folders.parent_id`; `SearchFailure` names the
  account's kind.
- The HTML-to-text renderer moved from `provider/gmail/html.rs` to `provider/html.rs`.
- A provider whose first sync is its delta defers the full backfill's sweep until the delta has
  caught up, and a larger `history_days` resyncs it.
- `rail doctor` and the token-refresh error now point at `rail account login <name>` instead of
  removing the account from `config.toml` and adding it again.
- `rail account add` flags are optional: with none (or only `gmail`/`m365`) the wizard opens. The
  flag form is unchanged for scripts, except that a partial set (`--address` without `--client-id`,
  or any other flag without `--address`) exits 1 with a message naming the missing flag.
- A missing config file now suggests `rail setup`.
- The roadmap gains Phase 2 (app shell and onboarding); sync and everything after it move back one
  phase.
- `rail account add`, `rail account login`, `rail account list` and `rail doctor` run through the
  daemon. `rail doctor` ends with the daemon's line and reports each account's `ai_send` policy.
- The setup wizard sends its actions to the daemon as requests; `Esc` or `Ctrl-C` cancels a login
  waiting there, and a save already begun is never interrupted.
- `rail account list` sizes its columns to the longest name and address, and `rail doctor` says
  "1 account".
- A preset file that does not parse is reported by path, line and column ("not a valid Rail
  preset") instead of the parser's message, and preset files over 64 KiB are refused.
- The Microsoft setup text names the *Application (client) ID* field, says single-tenant apps
  must give the tenant, and lists admin consent during a normal login.
- The roadmap gains Phase 3 (daemon, operations and MCP); Gmail sync and everything after it move
  back one phase.
- The daemon stays up while any account is configured; the ten-minute idle exit now applies only
  with no accounts.
- The roadmap marks Phase 4 done; display-quality HTML rendering moves to Phase 5.
- `rail` with a config no longer exits 2 with "the TUI is not built yet": it opens the mail screen
  (or the setup wizard when no account is configured). In a pipe it exits 2 with a hint.
- Message text from HTML now carries `[n]` link markers, and plain-text bodies carry them after
  bare addresses; bodies stored before this version show no markers until they are fetched again.
- The setup wizard's keys are actions of the keymap (`wizard.*`); its behaviour is unchanged,
  except that Ctrl and Alt combinations no longer act as the plain key.
- `rail doctor` notes a `general.layout` of `split` or `stack`, which this version treats as
  `auto`.
- The roadmap marks Phase 5 done; actions and compose are next.
- Plain-word search no longer matches every message for the words `name`, `address` or `null`:
  the full-text index holds the names and addresses of senders and recipients as plain text.
  Store migration 5 rebuilds the index once, the first time the daemon opens the store after the
  upgrade (this can take a moment on a large mailbox); until then the mail screen says the store
  is not set up.
- The `g x` confirmation shows its warnings first, then where the link really goes (the host, cut
  at the left so its end shows, or every `mailto:` recipient: `To`, `Cc` and `Bcc`, at most five
  lines and then "... and K more recipients"), then the address, cut if it is long. A warning
  names the whole host when the address has a user name or password before the `@`
  (`https://bank.com@evil.example`). A `mailto:` link with a parameter other than `to`, `cc`, `bcc`,
  `subject` or `body` (such as `attach=`) gets a first-line warning naming up to five of them. The
  yes/no hint always stays on the last row.
- Help and `:errors` show a `↓ more` / `↑ more` line (`v more` / `^ more` with `icons = "ascii"`)
  when there is more to scroll, and help keeps the `* set in config.toml` legend in its title;
  `:errors` scrolls with `j`/`k` or the arrow keys. Help closes on the bound help key, `Esc` or `q`.
- `rail search` retries a failing Gmail once, after a short wait (at most 2 seconds), so a Gmail
  outage fails fast; background sync keeps its retries.
- `rail sync` says "sync restarted in the daemon" when the daemon restarts its sync, and keeps
  "the daemon restarted" for a new daemon.
- The README notes that Firefox installed as a snap cannot read `o`'s files under `~/.cache` (set
  `RAIL_CACHE_DIR` to a folder it can read, or use a browser that is not a snap) and that a
  message's `<meta http-equiv="refresh">` can still move the page `o` opens.

### Fixed

- A Microsoft 365 folder whose sync link stayed longer than 4 KiB started over from its first page
  on every pass, forever. After three start-overs in a row Rail stops syncing it until the daemon
  restarts and, until then or until the folder is deleted in Outlook, says so in the header's failed
  state and in `:errors` (`a folder cannot be synced: its sync link is longer than 4 KiB`, with the
  folder's name after it as outside text). A resync while a folder is given up on, or while a
  subfolder listing is skipped, no longer deletes that folder's mail. A permanent sync error is now
  kept in `:errors` (#28).
- A Microsoft 365 folder whose own name holds `/` showed as two levels; the `/` now shows as `∕`.
  Once Rail has listed the folders since the daemon started, a folder whose subfolders vanished
  while Rail listed them no longer fails the whole sync pass, and an action on a conversation whose
  messages left the store says so instead of reporting success (#31).
- Undoing a Microsoft 365 move that a sync had overtaken could put the message back in two
  folders until its next sync; it goes back to the one folder it left. An undo no longer puts a
  message into a folder that took a deleted folder's place in the store (#33).
- `m` on a Microsoft 365 account with no synced folders said `This account has no labels to
  choose from.`; it now says `This account has no folders to choose from.` (#34).
- A Microsoft 365 draft's attachment overwritten in place with new bytes of the same length kept
  the old copy in Outlook, and that copy was sent. Rail now records each file's modification time
  and uploads a file whose time or size changed (#42).
- A reply or forward of an HTML mail quoted its `[1]` link markers without the links. The links
  the text marks now follow it, one line each (`[1] https://…`, or `(link not included)` for an
  address Rail would not open) (#43).
- When Rail could not take the terminal back from the editor, the next frame drew only what
  changed, so the editor's text could stay on screen. It now repaints the whole screen and logs
  the error (#23).
- `:thread-order` with the reader's cursor on a rule or a blank line moved the cursor to
  another message; it stays on the message it was in.
- A reader 15 to 18 columns wide showed `…  /  4 messages` under the subject (or, at 15, lost
  the count); it shows `/  4 messages`. A title or card line full exactly at a part's end now
  ends in `…` when something was left off. The From:, To: and Cc: lines no longer repeat an
  address that is also the name.
- Page down and page up in the thread list could leave threads between two screens unseen when
  the next screen had more group rows; a page now moves by the threads shown, the next screen
  starts with the thread after the last one shown and the screen above ends with the thread
  before the first one shown, whatever row the cursor is on (it goes to the new screen's last
  thread when that screen holds fewer). A group's name is no longer drawn alone on the bottom
  row.
- A thread dated beyond what the clock can read was grouped under `OLDER` even when it was in
  the future, so `OLDER` could head the list twice; it goes by its sign now.
- The header showed an unread count on an account's own Sent, Trash or Drafts folder
  (`work · Sent · 3 unread`); it reads `work · Sent`, as the merged mailboxes do.
- `:hints` turning the hints back on posted `hints: on` for five seconds over the hints it had
  just brought back; it says nothing, and a `hints: off` still showing goes with it (#45).
- The header's and the hint bar's backfill counts have thousands separators
  (`⟳ Backfill 1,200/5,000`, `home ⟳ 1,200/5,000`).
- A filter being typed or set keeps at least 24 cells of the header's search slot (at 60 columns
  and wider); the scope is cut to make the room, the summary and the clock stay whole.
- Microsoft 365 marking read touched every stored message of a thread, and one message Graph
  refused failed the whole action. Only the messages that need the change are sent, a refused one
  is skipped and logged by id, and the action fails only when every message is refused (#30).
- A 410 from Microsoft Graph outside a delta request was read as an expired delta and resynced
  the account; it is now a refusal. A throttled item inside a `$batch` waits for its own
  `Retry-After` (at most 30 seconds, one second when it names none) before it is sent again (#31, in part).
- A send or a save to Gmail whose review was closed, followed by a new message or reply before
  the daemon answered, lost its answer: no `sent · z undoes` record and no error when it failed.
  Its answer now makes its record (or shows its error) whatever is open. A thread action whose
  answer lands after a newer action now still puts a failed call's selection back and undoes its
  rows if `z` was pressed for it (saying `too late to undo` when it was); `z` still undoes only the
  newest action. While a send or save to Gmail waits for the daemon, its draft does not open from
  the Drafts folder or `:drafts`, its review refuses `e`, `a` and `x`, and a lost connection says
  `Could not confirm the send: … · check Sent and Drafts` instead of leaving `y` and `s` waiting.
- A sent imported draft's attachment files are removed when the send completes, not only at the
  next daemon start (best effort; the start sweep still catches what a removal could not).
- A send that fails on an attachment (a file that cannot be read, a forwarded attachment gone from
  Gmail or the store) no longer quotes the attachment's file name in the error, so a name from
  outside Rail is not shown unlabelled or split at `: `.
- A daemon left running from an earlier build of the same version was kept, and refused new
  operations with `no operation named "draft.push"`. Each build now carries the time `rail-core` was last built
  and sends it in the hello; a client replaces a daemon from an earlier build (or one that sends
  none), and refuses one from a later build with "restart this program". A call the daemon does
  not know now hints `rail daemon stop`, and `rail daemon status` shows the daemon's build.
- Importing a Gmail draft with attachments, and forwarding a message with its attachments, failed
  with "Invalid attachment token"; Rail now fetches an attachment by Gmail's own attachment id, an empty attachment
  imports as an empty file, and an attachment gone from Gmail says so.
- The "Rail was upgraded" hint shows only the daemon's `MAJOR.MINOR.PATCH`, and Rail's own error
  text is escaped on the terminal.
- `rail secrets set` refuses control and format characters in `--vault`/`--account`, as MCP did.
- Account addresses refuse Unicode format characters (bidi overrides, zero-width spaces).
- A label renamed in Gmail keeps the sidebar cursor, and the header keeps its unread count and shows
  the new name; a deleted label sends the screen to its account's inbox. A label beyond the
  500-per-account sidebar cap gets its own notice ("more than 500 labels") instead of "gone".
- A failed sidebar read shows in the sidebar and a failed thread read in the reader, and both reach
  the status bar and `:errors`; only a failed list read replaces the list.
- After a message fetch gets through to the daemon again, the status bar says "daemon reachable
  again" instead of keeping "daemon unreachable"; a later fetch that finds the daemon gone again
  puts "daemon unreachable" back.
- The first `j` in help and `:errors` after the top shows a new row (the `↑ more` line used to take
  the row it would have revealed).
- Upgrading a store whose full-text triggers or table were already missing no longer fails.
- An HTML answer for a message outside the open thread is dropped; the reader's cursor is clamped
  when the body shrinks; a re-read of the open thread no longer moves the focus; a refresh ends
  only when the refresh itself ends, even if it panics.
- The palette and the filter line draw the cursor where it is, so editing in the middle of the
  text shows where you are typing.
- The 500-folder sidebar cap applies per account, so one account's labels cannot hide another's.
- The terminal is restored once per session, whichever of the panic hook, the guard or a signal
  gets there first; SIGHUP and SIGTERM end Rail normally after the screen has closed, and stay
  graceful while it runs.
- `Retry-After` given as an HTTP date is honoured (capped, as a number of seconds already was; a
  date in the past retries now); concurrent requests refused with a 401 for one account share one
  token refresh, and a request makes at most three authorised attempts.
- `rail daemon install` on macOS uses the uid of the user running it, not the owner of `HOME`.
- The daemon notices a `config.toml` edit that keeps the file's modification time: it compares a
  digest of the contents as well.
- A store the daemon is still migrating is reported as a temporary error where an engine's sync
  status lines or a search meet it, instead of a failure; `rail doctor`'s store line still fails on
  any store error, and a newer store is still an error.

### Security

- Tokens are redacted from `Debug` output and never reach logs, errors or the terminal.
- Database, secrets and config files are created owner-only and written atomically.
- The loopback listener verifies the OAuth `state` before honouring a code or an error redirect
  and escapes provider-supplied text on its error page.
- The wizard never starts when stdout is not a terminal (it prints a hint and exits 2), and nothing
  prints while it owns the screen. Client secrets are masked on screen and redacted from `Debug`
  output along with authorization codes.
- The loopback listener ignores admin-consent redirects whose `state` is absent or wrong, as it
  already did for login redirects.
- Tokens, client secrets and authorization codes never reach a daemon response, an MCP tool result,
  a log line or an `op` command line: 1Password values go through templates on `op`'s stdin and
  `op`'s errors are redacted.
- After `op item edit` replaces an account's tokens, Rail reads the item back; if 1Password did
  not keep the new values, the save fails ("1Password did not keep the new tokens; nothing was
  saved") instead of reporting success.
- No MCP tool can choose the program Rail runs: `op_path` is set only by hand in `config.toml`, and
  Rail refuses one that is relative, not an executable file, owned by another user, or writable by
  group or others, or inside a directory that is.
- Files an agent names (a client secret JSON, a preset path) must be absolute and are read with a
  64 KiB cap; a file that does not parse is never echoed back.
- Through MCP, an account removal or a secret-store change happens only after the user confirms
  in the client's dialog; no tool input confirms for the user. An agent that can run shell
  commands could drive `rail` (or the socket) as the user, and Rail cannot stop a program running
  as the same user: the control there is the agent client's own command approval. The
  confirmation text is written by the daemon in plain ASCII (non-ASCII as `\u{..}`), never cut, and
  bound to a digest of the plan; if anything changed in between, nothing runs.
- Text from outside Rail (presets, provider and system error text) reaches the agent marked as
  untrusted, quoted on one line, capped at 200 characters and behind a notice that it is data; a
  provider's error text is kept apart from Rail's own message.
- An account's address, colour and tenant may not contain spaces or control characters, at add
  and when `config.toml` loads (with path and line), so none can carry a terminal escape.
- `store_secret_in_1password` is refused for a client secret that is already an `op://`
  reference, so Rail never copies an item a request named into its own vault.
- Preset files and client secret files are read through one guarded open: only a regular file,
  checked before and after opening, never blocking on a FIFO, at most 64 KiB.
- A daemon answer too large to send becomes an internal error for the same request instead of
  leaving the caller waiting.
- `rail mcp` logs the MCP library only at error level, because its warnings can quote what an agent
  sent.
- Mail is hostile input: header values are flattened to one line and capped, decoded words are
  capped, HTML is scanned by Rail's own linear scanner (no parser tree) that runs nothing and
  fetches nothing, and a body over 10 MiB is stored cut with a marker.
- Sync requests are pinned to the Gmail API origin, carry no mail text in URLs logged at debug
  (no query strings), and are paced under Gmail's quota with retries on rate limits.
- No MCP result carries mail content: no subject, sender, snippet or body, and Gmail's error text
  reaches agents only as outside text.
- `rail search` and sync output escape control and invisible formatting characters.
- The login unit keeps display, browser and 1Password variables out of the daemon where it can,
  and `rail daemon install` quotes paths for systemd and XML-escapes them for launchd.
- Every string from mail, Gmail or config that reaches the mail screen is escaped (controls,
  format characters, line separators), with width measured after escaping. Links are shown and
  opened only for `http`, `https` and `mailto` addresses, and only after their address is shown.
- `o` writes the HTML 0600 with a policy that blocks scripts, deletes it when the reader closes,
  and Rail itself never loads remote content.
- A link is refused when its normalised address (percent-encoded, punycode) is over 2,048 bytes, not
  only when the text in the message is.
- While a box such as help or a confirmation is open, the reader draws plain `[n]` link markers,
  so a box border can never land on a hyperlink cell.
- A panicking background job or the setup wizard's job logs the length of the panic message, never
  its text, which can quote mail or a token.
