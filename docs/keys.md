# Keys

Every key Rail reads, how to change one, the key presets, and the full list of actions.

- [Actions and `[keys]`](#actions-and-keys)
- [Key names](#key-names)
- [When Rail refuses a `[keys]` entry](#when-rail-refuses-a-keys-entry)
- [Key presets](#key-presets)
- [Where actions work](#where-actions-work)
- [Every action](#every-action)

The setup wizard's keys are described with the wizard in
[Account setup](accounts.md#the-setup-wizard); they are the `wizard.*` actions below.

## Actions and `[keys]`

Every key Rail reads is an *action* with an id and a default key. Change one in `config.toml`:

```toml
[keys]
"list.filter" = "/"
"reader.open_browser" = "O"
"global.palette" = ":"
```

A key you set replaces that action's default keys.

## Key names

A key name is one of:

| Form | Examples |
|---|---|
| A character | `a`, `O`, `/` |
| A named key | `enter`, `esc`, `tab`, `backtab`, `space`, `up`, `down`, `left`, `right`, `pageup`, `pagedown`, `home`, `end`, `f1` to `f12` |
| Either of those with `ctrl-`, `alt-` or `shift-` in front | `ctrl-r`, `shift-tab`, `shift-down` |
| Two keys with a space between them, for a sequence | `g x` |

`shift-` goes on a letter, `tab`, `up` or `down`.

## When Rail refuses a `[keys]` entry

Both `rail` and `rail setup` refuse to start, naming the file and line and exiting 1, when
`[keys]` names:

- an action it does not know;
- a key it cannot read;
- `Ctrl-C`, which always quits;
- a key another action of the same view already uses. A key that starts another action's
  sequence counts.

They check before the terminal changes.

A view's own key wins over a global one: `q` closes the reader but quits from the list.

The defaults count too:

- `J`, `K`, `shift-down` and `shift-up` extend the selection in the list. A `[keys]` entry that
  binds one of them to another list action (`"list.down" = "J"`, say) stops Rail from starting
  with that error until you change the entry or move `list.extend_down` / `list.extend_up` to
  other keys.
- `h` snoozes in the list and the reader, so the reader's headers moved to `H`. An older
  `"reader.headers" = "h"` entry stops Rail from starting with
  `[keys] "list.snooze" (h) and "reader.headers" (h) cannot share a key in the reader view` until
  you remove it or bind `list.snooze` to another key.

## Key presets

`keymap` under `[general]` chooses the keys every action starts from. Your `[keys]` entries go
over the preset you choose.

A preset changes keys only while you use it. Rail's own keys, the `rail` preset, stay as they
are: choosing or adding a preset never moves them.

| Preset | Keys |
|---|---|
| `rail` (the default) | The keys in [the action table](#every-action). |
| `vim` | The table's keys, except those listed [below](#the-vim-preset). |
| `mutt` | The table's keys, except those listed [below](#the-mutt-preset). |
| `emacs` | The table's keys, plus those listed [below](#the-emacs-preset). |

### Switching presets

`:keymap` in the palette lists the presets with what sets each apart. It switches as soon as you
press `Enter`, and saves the choice.

A switch your own `[keys]` entries clash with is refused, with the message Rail would give at
start-up, and the keys you had stay.

`F2` opens the palette in every preset, whatever `[keys]` says, so a switch never hides the way
back (`keys: vim · F2 opens the palette`). `[keys]` cannot bind `F2` to another action.

### The vim preset

| Keys | vim preset |
|---|---|
| `h` / `l` | Go out / in: `l` opens a thread (list) or a mailbox (sidebar) and unfolds quoted text (reader); `h` goes to the sidebar (list), back to the list (sidebar) or closes the thread (reader). `Tab`, `Enter`, `q` and `Esc` work as before. |
| `u` | Undo (`z` is unbound). |
| `U` | Mark read or unread (`u` in `rail`). |
| `Z` | Snooze (`h` in `rail`). |
| `L` | Labels (`l` in `rail`). |
| `D` | Move to the trash without asking (`list.trash_now`); `d` still asks, and undo still works. In Drafts it asks too, since a discarded draft cannot be undone. |
| `Ctrl-f` / `Ctrl-b` | One page down / up, beside `PageDown`/`PageUp` and `Ctrl-d`/`Ctrl-u` (also in the reader and the composer). |
| `N` | Previous message in the reader, beside `p`. |

### The mutt preset

Keys follow mutt's index and pager. `d` still asks before trashing, as in `rail`.

| Keys | mutt preset |
|---|---|
| `s` | Move to a label or folder (mutt's save; `m` in `rail`). |
| `F` | Star or unstar (mutt's flag; `s` in `rail`). |
| `N` | Mark read or unread (mutt's toggle-new; `u` in `rail`). |
| `u` | Undo (`z` is unbound). |
| `D` | Move to the trash without asking (`list.trash_now`); `d` still asks, and undo still works. In Drafts it asks too. |
| `t` | Select or unselect the thread (mutt's tag), beside `Space`. |
| `T a` / `T f` | Select every matching thread / every thread from this sender (`* a` / `* f` in `rail`). |
| `l` | Filter the list (mutt's limit), beside `/`. |
| `g` | Reply to everyone (mutt's group reply), beside `R`. |
| `m` | Write a new message (mutt's mail), beside `c`. |
| `$` | Sync now, beside `Ctrl-r`. |
| `=` / `*` | Newest / oldest thread, beside `Home` / `End` (`g g` / `G` in `rail`). In the reader `=` goes to the first line. |
| `Ctrl-o` | Open the link the subject names (`g t` in `rail`). |
| `Z` | Snooze (`h` in `rail`). |
| `Y` | Labels (neomutt's edit-label; `l` in `rail`). |
| `h` | Show or hide the headers in the reader (mutt's toggle-weed; `H` in `rail`). |
| `i` | Close the thread in the reader (mutt's exit from the pager), beside `q` and `Esc`. |
| `v` | List the attachments in the reader (mutt's view-attachments; `a` in `rail`). |
| `P` | Load the thread's remote pictures in the reader (`i` in `rail`). |
| `Ctrl-b` | Open a link in the reader (`g x` in `rail`). |
| `-` | One page up in the reader, beside `PageUp`. |
| `D` (composer) | Remove the highlighted attachment (mutt's detach), beside `x`. |

`g` and `*` are single keys here, so no other key starts with them: that is why the top, bottom,
link and select-all keys move.

### The emacs preset

Keys follow emacs's movement keys and its `Ctrl-x` prefix. The preset only adds keys: every key
in the `rail` preset still works.

| Keys | emacs preset |
|---|---|
| `Ctrl-n` / `Ctrl-p` | Next / previous line or item (list, sidebar, reader, composer). |
| `n` / `p` | Next / previous thread in the list, as in mu4e and Gnus. In the reader they still go to the next and previous message. |
| `Ctrl-v` / `Alt-v` | One page down / up (list, sidebar, reader, composer). |
| `Alt-<` / `Alt->` | Newest / oldest thread in the list; first / last line in the reader. |
| `Ctrl-s` | Filter the list, beside `/`. |
| `Ctrl-g` | Back out: clear the selection (list), close the thread (reader), close the composer and keep the draft. |
| `Alt-x` | Open the command palette, beside `:` and `F2`. |
| `Ctrl-x u` | Undo, beside `z`. |
| `Ctrl-x o` | Move between the sidebar and the list, beside `Tab`. |
| `Ctrl-x Ctrl-s` | Save to Drafts in the composer, beside `s`. |
| `Ctrl-x i` | Attach a file in the composer, beside `a`. |
| `Ctrl-x k` | Discard the draft in the composer (asks first), beside `d`. |

`Ctrl-c` quits Rail in every preset, so emacs's `Ctrl-c` chords (`Ctrl-c Ctrl-c` to send) are not
bound: send with `y`. `Alt-` keys reach Rail as `Esc` followed by the key in most terminals; on
macOS, set the terminal's Option key to send Esc+ (iTerm2: Profiles → Keys; Terminal: "Use Option
as Meta key").

## Where actions work

| Actions | Take effect in |
|---|---|
| `global.*`, `list.*`, `sidebar.*`, `reader.*`, `compose.*` | The mail screen |
| `wizard.*` | The setup wizard |

In the mail screen `?` lists the keys of the view you are in, with `*` after the ones set in
`config.toml`.

## Every action

The *Default keys* are those of the `rail` preset. *Notes* adds context that is not part of the
action's own description.

<!-- actions:begin -->
| Action | Works in | Default keys | What it does | Notes |
|---|---|---|---|---|
| `global.quit` | global | `q` | Quit Rail. |  |
| `global.palette` | global | `:` `f2` | Open the command palette. | `F2` stays whatever `[keys]` says. |
| `global.help` | global | `?` | Show the keys for this view. |  |
| `global.undo` | global | `z` | Undo the last action. |  |
| `list.open` | list | `enter` | Open the thread. |  |
| `list.filter` | list | `/` | Filter the list as you type. |  |
| `list.save_search` | list | `S` | Save the filter as a search. |  |
| `list.sidebar` | list, sidebar | `b` | Show or hide the sidebar. |  |
| `list.account_1` | list, sidebar | `1` | Go to account 1's inbox. |  |
| `list.account_2` | list, sidebar | `2` | Go to account 2's inbox. |  |
| `list.account_3` | list, sidebar | `3` | Go to account 3's inbox. |  |
| `list.account_4` | list, sidebar | `4` | Go to account 4's inbox. |  |
| `list.account_5` | list, sidebar | `5` | Go to account 5's inbox. |  |
| `list.account_6` | list, sidebar | `6` | Go to account 6's inbox. |  |
| `list.account_7` | list, sidebar | `7` | Go to account 7's inbox. |  |
| `list.account_8` | list, sidebar | `8` | Go to account 8's inbox. |  |
| `list.account_9` | list, sidebar | `9` | Go to account 9's inbox. |  |
| `list.unified` | list, sidebar | `0` | Go to the unified inbox. |  |
| `list.refresh` | list, sidebar, reader | `ctrl-r` | Sync now; reconnect to the daemon. |  |
| `list.down` | list | `j` `down` | Next thread. |  |
| `list.up` | list | `k` `up` | Previous thread. |  |
| `list.page_down` | list | `pagedown` `ctrl-d` | One page down. |  |
| `list.page_up` | list | `pageup` `ctrl-u` | One page up. |  |
| `list.top` | list | `home` `g g` | Newest thread. |  |
| `list.bottom` | list | `end` `G` | Oldest thread. |  |
| `list.archive` | list, reader | `e` | Archive the thread or the selection. |  |
| `list.trash` | list, reader | `d` | Move to the trash (asks first). |  |
| `list.trash_now` | list, reader | (none) | Move to the trash without asking (undo still works). | `D` in the `vim` and `mutt` presets. |
| `list.star` | list, reader | `s` | Star or unstar. |  |
| `list.read` | list, reader | `u` | Mark read or unread. |  |
| `list.labels` | list, reader | `l` | Add or remove labels (Gmail). |  |
| `list.move` | list, reader | `m` | Move to a label (Gmail) or a folder (Microsoft 365). |  |
| `list.snooze` | list, reader | `h` | Snooze until later, or return a snoozed thread. | See [Snooze](actions.md#snooze). |
| `list.compose` | list, reader | `c` | Write a new message. |  |
| `list.open_link` | list, reader | `g t` | Open the link the subject names. | The thread's subject, through [`[[link_actions]]`](configuration.md#link_actions). |
| `list.reply` | list, reader | `r` | Reply to the latest message. |  |
| `list.reply_all` | list, reader | `R` | Reply to everyone. |  |
| `list.forward` | list, reader | `f` | Forward the latest message. |  |
| `list.select` | list | `space` | Select or unselect the thread. |  |
| `list.clear_selection` | list | `esc` | Clear the selection. |  |
| `list.select_all` | list | `* a` | Select every thread this mailbox and filter match. |  |
| `list.select_sender` | list | `* f` | Select every thread from this sender. |  |
| `list.select_range` | list | `v` | Select a range of threads. |  |
| `list.extend_down` | list | `J` `shift-down` | Extend the selection down. |  |
| `list.extend_up` | list | `K` `shift-up` | Extend the selection up. |  |
| `sidebar.focus` | list, sidebar | `tab` | Move between the sidebar and the list. |  |
| `sidebar.down` | sidebar | `j` `down` | Next mailbox. |  |
| `sidebar.up` | sidebar | `k` `up` | Previous mailbox. |  |
| `sidebar.page_down` | sidebar | `pagedown` `ctrl-d` | One page down. |  |
| `sidebar.page_up` | sidebar | `pageup` `ctrl-u` | One page up. |  |
| `sidebar.open` | sidebar | `enter` | Show this mailbox. |  |
| `sidebar.fold` | sidebar | `space` `left` `right` | Fold or unfold this account or folder. |  |
| `sidebar.rename_search` | sidebar | `r` | Rename this saved search. |  |
| `sidebar.remove_search` | sidebar | `d` | Remove this saved search (asks first). |  |
| `reader.next` | reader | `n` | Next message. |  |
| `reader.prev` | reader | `p` | Previous message. |  |
| `reader.expand` | reader | `tab` | Expand or collapse the message. |  |
| `reader.unfold` | reader | `enter` | Show the quoted lines, or open the message. |  |
| `reader.headers` | reader | `H` | Show or hide the headers. |  |
| `reader.open_browser` | reader | `o` | Open the message in the browser. |  |
| `reader.attachments` | reader | `a` | List the attachments. |  |
| `reader.save_attachments` | reader | `w` | Save this message's attachments. |  |
| `reader.load_images` | reader | `i` | Load this thread's remote pictures. |  |
| `reader.trust_images` | reader | `I` | Always load this sender's remote pictures, or stop. |  |
| `reader.follow_link` | reader | `g x` | Open a link. |  |
| `reader.back` | reader | `q` `esc` | Close the thread. |  |
| `reader.down` | reader | `j` `down` | Next line. |  |
| `reader.up` | reader | `k` `up` | Previous line. |  |
| `reader.page_down` | reader | `pagedown` `space` | One page down. |  |
| `reader.page_up` | reader | `pageup` | One page up. |  |
| `reader.top` | reader | `home` `g g` | First line. |  |
| `reader.bottom` | reader | `end` `G` | Last line. |  |
| `compose.send` | compose | `y` | Send. |  |
| `compose.push` | compose | `s` | Save to Drafts (Gmail or Microsoft 365). |  |
| `compose.edit` | compose | `e` | Edit in your editor. |  |
| `compose.headers` | compose | `t` | Change To, Cc, Bcc or Subject. |  |
| `compose.attach` | compose | `a` | Attach a file. |  |
| `compose.remove` | compose | `x` | Remove the highlighted attachment. |  |
| `compose.close` | compose | `q` `esc` | Close and keep the draft. |  |
| `compose.discard` | compose | `d` | Discard the draft (asks first). |  |
| `compose.down` | compose | `j` `down` | Next line. |  |
| `compose.up` | compose | `k` `up` | Previous line. |  |
| `compose.page_down` | compose | `pagedown` `space` | One page down. |  |
| `compose.page_up` | compose | `pageup` | One page up. |  |
| `wizard.back` | wizard | `esc` | Go back one step. |  |
| `wizard.portal` | wizard | `f1` | Open the provider portal or the preset's help. |  |
| `wizard.next_field` | wizard | `tab` | Next field. |  |
| `wizard.prev_field` | wizard | `backtab` | Previous field. |  |
| `wizard.next` | wizard | `enter` | Choose or continue. |  |
| `wizard.up` | wizard | `up` `k` | Move up. |  |
| `wizard.down` | wizard | `down` `j` | Move down. |  |
| `wizard.page_up` | wizard | `pageup` | Scroll one page up. |  |
| `wizard.page_down` | wizard | `pagedown` | Scroll one page down. |  |
| `wizard.top` | wizard | `home` | Scroll to the top. |  |
| `wizard.bottom` | wizard | `end` | Scroll to the end. |  |
| `wizard.open_portal` | wizard | `o` | Open the portal from the instructions. |  |
| `wizard.toggle` | wizard | `space` | Tick or untick the checkbox. |  |
| `wizard.retry` | wizard | `r` | Retry after an error. |  |
| `wizard.copy_url` | wizard | `c` | Copy the URL shown. |  |
<!-- actions:end -->

