# Changelog

All notable changes to Rail are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

Generic IMAP and SMTP provider support: connect standard-compliant mail servers alongside
Gmail and Microsoft 365, with secure password storage across OS keyring, private files, and
1Password.

### Added

- **Generic IMAP and SMTP provider.** Rail now supports standard IMAP and SMTP mailboxes alongside
  Gmail and Microsoft 365, with automatic mailbox discovery (RFC 6154 special-use attributes),
  TLS modes (`required`, `opportunistic`, `off`), and fallbacks for servers without `MOVE`,
  `UIDPLUS`, or `IDLE`.
- **Password credentials management.** Added secure password storage in `SecretStore` across the
  OS keyring, private files, and 1Password. Passwords are never stored in `config.toml`, are masked
  without echo during terminal input, and are redacted at the type level (`Secret<String>`).
- **Setup wizard & CLI support for IMAP/SMTP.** The setup wizard (`rail setup`) supports
  adding and testing generic IMAP/SMTP accounts with interactive password prompts. The CLI supports
  adding IMAP/SMTP accounts via `rail account add imapsmtp` and managing stored passwords via
  `rail secrets password <account> [--service <imap|smtp>]`.
- **Two-phase send for SMTP.** Outgoing mail is sent via SMTP relay and subsequently appended to
  the IMAP Sent mailbox. Transient append failures are retried without re-dispatching to SMTP,
  preventing duplicate deliveries.
- **MCP agent guardrails.** MCP tools (`rail_account_add`, etc.) reject pasted passwords and
  disallow configuring IMAP/SMTP accounts from agents to preserve credential security.
- **A Nix flake.** `nix run github:vitosans/rail` runs Rail without installing it, `nix profile
  install` puts it on the path, and `nix develop` opens the development shell with the toolchain
  from `rust-toolchain.toml` plus `cargo-deny` and `cargo-llvm-cov`. `nix build` packages the
  binary; the flake pins nixpkgs and rust-overlay in `flake.lock`.

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
