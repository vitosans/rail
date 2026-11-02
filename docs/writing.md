# Writing mail

New messages, replies and forwards: the editor, the review, sending with undo, drafts on this
computer and in the account's Drafts, and the limits.

- [Starting a message](#starting-a-message)
- [A new message](#a-new-message)
- [The editor](#the-editor)
- [The review](#the-review)
- [Sending](#sending)
- [Drafts](#drafts)
- [The Drafts folder](#the-drafts-folder)
- [Files of an imported draft](#files-of-an-imported-draft)
- [Limits](#limits)
- [Your address and signature](#your-address-and-signature)

Writing from a Microsoft 365 account works the same way;
[Microsoft 365 mail](accounts.md#microsoft-365-mail) says what differs.

## Starting a message

From the thread list or the reader:

| Key | Action |
|---|---|
| `c` | Write a new message. |
| `r` | Reply to the latest message of the thread. |
| `R` | Reply to everyone. |
| `f` | Forward it. |

- In the list they act on the thread under the cursor; in the reader, on the open thread.
- In the unified inbox with more than one account, `c` first asks which account to write from.
  Elsewhere it writes from the account you are looking at.
- One message is written at a time (`Finish or close the message being written first.`).

### Replies and forwards

- A reply goes to the message's `Reply-To` when it has one, otherwise to its sender.
- `R` adds the other recipients except you, keeping the old `Cc` in `Cc`.
- A reply starts with `Re:` once, and the quoted text under `On <date>, <name> wrote:`.
- A forward starts with `Fwd:`, no recipients, and the original under a `Forwarded message` line,
  with its attachments.
- The links the quoted or forwarded text marks follow it after a blank line, one per marker:
  `[1] https://…`, or `[2] (link not included)` for an address Rail would not open.
- If the message's text is not in the store yet, Rail fetches it first (`fetching the message…`).

## A new message

**A new message** starts with two prompts on the bottom row: `To:`, then `Subject:`. A reply or
forward skips the prompts and opens the editor at once.

`To:` completes from the addresses your mail has taught Rail. Type a few letters and matching
addresses appear above, none highlighted.

| Key | Action |
|---|---|
| `Tab` / `Shift-Tab` (or `↓` / `↑`) | Move through the matches. |
| `Enter` | Take the highlighted one, or the text you typed when none is. |
| `,` | Take the highlighted one and start the next recipient. With none highlighted it is typed as a comma. |
| `Esc` | Cancel the message, in either prompt; nothing is saved. |

Each address is checked as you press `Enter`, and the prompt stays open with the reason when one
is wrong.

A recipient is a plain `user@domain` or `Name <user@domain>`. A quoted name with a comma
(`"Reyes, Dana" <dana@example.com>`) is one recipient.

## The editor

Rail runs `general.editor`, else `$VISUAL`, else `$EDITOR`, else `vi`, on a file in its cache
folder (`compose/<draft>.eml`, mode 0600).

- The command is split like a shell command line (quotes work) but no shell runs, so pipes and `~`
  are not interpreted.
- An editor that returns at once needs its wait flag: use `editor = "code --wait"`.
- While the editor runs Rail steps aside and ignores `Ctrl-C` and `Ctrl-\`. It takes the terminal
  back when the editor exits.

### The file

The file holds four headers, a blank line and the body:

```
To: Dana Reyes <dana@example.com>
Cc:
Bcc:
Subject: Lunch

Are you free on Thursday?
```

- `To`, `Cc` and `Bcc` take comma-separated recipients.
- A header is one line; a line starting with a space continues the one before.
- Any other header, or a line that is not `Name: value`, is refused: Rail names the line and opens
  the editor again on the same text.
- Rail also accepts a file the editor saved with Windows line endings (CRLF) and keeps the message
  as plain lines.

### When the editor fails

- If the editor exits with a non-zero status Rail asks `Keep what you wrote?`: yes saves the text,
  no keeps the draft as it was.
- If the editor cannot start Rail says `cannot run …` and the review opens with the text it had.

### Recovering the editor's file

The file is kept until a save succeeds, and also when the editor cannot run or its file cannot be
read back.

- If Rail stops before that (a crash, a closed terminal), the next editor round for that draft
  opens on the file as it was left, headers included, and says
  `opened the text from your last editor session`. Changes made with `t` after reopening the draft
  are replaced by the file's headers.
- Sending or discarding the draft without opening the editor uses the stored text, and the file
  stays until it is removed after seven days.
- Quitting that editor with an error and answering no to `Keep what you wrote?` keeps the draft as
  it was.
- A new message is saved before its first editor round, so it is recovered like any other draft.
- A `new.eml` (left when that first save failed) is never reused for another message; it is
  removed after seven days.

## The review

When the editor returns, Rail shows the message as it will go:

1. `From`, `To`, `Cc` and `Bcc` (when used), `Subject`;
2. the attachments, with their count and total size;
3. the body, with long runs of quoted lines folded to `[n quoted lines]`.

A recipient with an international domain is followed by the domain's ASCII form, as `xn--`
addresses are what the mail system sees.

The status bar names the keys:

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

These are the `compose.*` actions in the [key table](keys.md#every-action) and can be changed.
While a `t` or `a` prompt is open the status bar shows the prompt's keys instead:
`Enter accept  Tab complete  Esc cancel`.

### When `y` and `q` are refused or ask

- `y` is refused with a reason when there is no recipient (`Add a recipient first`), while a save
  is pending, or after a save failed.
- If a save failed, `q` asks `Discard the unsaved changes?` and then says
  `unsaved changes discarded`.
- A message closed before anything was saved says `The message was not saved.`

## Sending

`y` hands the message to the daemon, which sends it ten seconds later.

- The status bar says `sent · z undoes`, and `z` takes it back inside that window: the review
  opens again with the message as it was.
- After the ten seconds the account shows `sending…` and the message goes through Gmail like the
  actions in [Doing things to mail](actions.md#how-actions-reach-the-server).

### Closing the review early

Closing the review with `q` before the daemon has the message does not stop the send. Its
`sent · z undoes` (or `Could not send`) still comes, even if another message is open by then. `z`
then cancels the send but cannot reopen its review; the draft stays in Drafts, to open from there.

### While a message is being queued

| What you try | Rail says |
|---|---|
| `y` or `s` on any message, until the last message is queued | `Wait for the last message to be queued.` |
| `e`, `a` or `x` on the review being sent or saved to the account's Drafts | Refused, as above. |
| Opening a draft whose send is on its way, from the Drafts folder or `:drafts` | `This draft is being sent; wait for it to be queued.` |
| Opening a draft whose save (`s`) to Gmail is on its way | `This draft is being saved to Gmail; wait a moment.` |
| The same on a Microsoft 365 account | `This draft is being saved to Microsoft 365; wait a moment.` |

Other drafts open as usual.

### If the connection to the daemon is lost

If the connection to the daemon is lost first, Rail cannot tell whether the message was queued:

| For | Rail says |
|---|---|
| A send | `Could not confirm the send: … · check Sent and Drafts` |
| `s` on Gmail | `Could not confirm the save to Gmail: … · check Drafts` |
| `s` on Microsoft 365 | `Could not confirm the save to Microsoft 365: … · check Drafts` |

If it is in Sent, it went. If it is still in Drafts, open it and press `y` again. That is safe,
since the daemon refuses a draft it already has queued (`this draft is already being sent`) and a
sent draft is gone.

### Never sent twice

A message is never sent twice. Before any later attempt (a retry, or a start after the daemon
stopped mid-send), the daemon asks Gmail whether a message with that `Message-ID` has the `SENT`
label and, if so, counts it as sent. An unsent copy in Gmail's Drafts does not count.

### Other outcomes

- A reply whose conversation was deleted in Gmail meanwhile goes out on a conversation of its own.
- When Gmail refuses a message for good, the review opens again with the error
  (`Could not send: Gmail refused the request (HTTP 400)`) and the failure is listed in `:errors`.

## Drafts

Every editor round, header change and attachment change saves the message in the local store, so a
crash or a closed terminal loses nothing. A reply whose original message has since left the store
keeps its threading.

### Saving to the account's Drafts

A draft stays on this computer until you press `s` on its review. Then Rail:

1. saves any text not stored yet;
2. saves the draft to Gmail's Drafts ten seconds later (Outlook's Drafts for a Microsoft 365
   account);
3. closes the review and says `saved to Gmail · z undoes` (`saved to Microsoft 365 · z undoes`).

`z` inside that window cancels it (`kept on this computer`).

- The review closes after `s`, so pressing `s` again means opening the draft again from the Drafts
  folder or `:drafts`. Done inside the ten seconds, it makes one save with the newest text, and
  `z` after it cancels that save.
- `s` is refused when the message holds nothing at all.
- The status bar shows `saving to Gmail…` (`saving to Microsoft 365…`) while a save is on its way.

### When a save fails

| When | Rail says |
|---|---|
| The provider refuses it on the spot (the review stays open) | `Could not save to Gmail: …`, or `Could not save to Microsoft 365: …` |
| It fails later (listed in `:errors`, where `Enter` retries it) | `Could not save the draft to Gmail: …`, or `Could not save the draft to Microsoft 365: …` |

### One draft in Gmail

- The first save creates the draft in Gmail and the next ones update it, so the draft keeps one
  place in Gmail's Drafts and opens on your phone.
- Rail's version wins: if the same draft was also edited in Gmail, the next `s` overwrites that
  edit.
- Sending a draft that was saved to Gmail deletes its Gmail copy once the message is sent. If that
  delete fails it is listed in `:errors`, where `Enter` retries it.

### A possible second copy

Creating the Gmail draft is tried once and never repeated inside one attempt, as a send is. When
that attempt may have reached Gmail but its answer was lost, the retry later can leave a second
copy of the draft in Gmail's Drafts.

Delete the extra one there. Updating a draft, deleting it and listing drafts are retried freely and
cannot do this.

## The Drafts folder

Each account's `Drafts` in the sidebar lists its drafts in one pane (`Drafts` under UNIFIED lists
every account's), newest first, up to 500: the drafts saved on this computer and the Gmail drafts
written elsewhere (on your phone, in the browser).

### What a row shows

- A row shows the date, the recipients and the subject, like a thread.
- A draft kept only on this computer says `(local)` where the recipients go and has `✎` (`*` with
  ASCII icons) before its subject.
- A draft saved to Gmail is listed once.
- A Gmail draft Rail already holds that was edited elsewhere since is listed again as a Gmail row;
  opening it opens Rail's copy, as Rail's version wins.

### Opening and discarding

- `Enter` opens a draft at its review. A Gmail draft is fetched first (`fetching the draft…`),
  together with its attachments, and from then on is a draft of this computer too, its Gmail copy
  updated by `s`.
- `d` asks `Discard this draft? (y/n)`. Yes deletes it, and its copy in the account's Drafts with
  it (Outlook moves it to Deleted Items).
- While a save to Gmail is still on its way the discard is refused
  (`draft: saving to Gmail, try again in a moment`, or
  `draft: saving to Microsoft 365, try again in a moment`): try again once it has landed.

| When | Rail says |
|---|---|
| The Gmail draft was deleted elsewhere before you opened or discarded it | `Gmail no longer has this draft` |
| It cannot be fetched | `Could not open the draft: …` or `Could not discard the draft: …`, with the reason |

### Keys that do not apply

Archive, star, read, labels, move, select, selecting all or by sender, the range, reply, forward
and the filter (`e`, `s`, `u`, `l`, `m`, `space`, `* a`, `* f`, `v`, `r`, `R`, `f`, `/`) say
`Not available on drafts`. `Esc` does nothing there, and `c` writes a new message as everywhere.

The search slot is empty in this folder: the pane is not filtered. The pane is read again whenever
sync or the outbox changes something.

### `:drafts`

`:drafts` (`Open a draft` in the palette) lists the same rows for the account in view. In the
unified inbox and the merged mailboxes it merges every account's, and `Enter` opens one the same
way.

### Microsoft 365 drafts

A Microsoft 365 account's rows are its Outlook drafts and behave the same way:

- `Enter` imports one with its attached files;
- `d` says `It is deleted from Outlook's Drafts.` and deletes it there;
- one deleted in Outlook first says `Microsoft 365 no longer has this draft`.

## Files of an imported draft

The attachments of a Gmail draft are fetched once, when you open it, into private files the daemon
keeps in the `draft-files` folder of the data directory (folders 0700, files 0600).

- Later saves and the send use those files, not Gmail's copy.
- Discarding the draft removes its files, and so does a completed send.
- When the daemon starts it removes every file no draft refers to, which catches any a removal
  could not.

## Limits

| Limit | Value |
|---|---|
| Recipients | At most 100 |
| Attachments | At most 50 |
| Total size | 25 MiB in all (Gmail's limit, kept for Microsoft 365) |
| Text | 10 MiB |
| Subject | One line |

- Attachments count as they are sent, a third larger than the files (so about 18 MiB of files
  fit). The review's total shows the files' own sizes.
- A file that would take the message over is refused when you attach it, naming the size the
  message would have.
- File names are cleaned before they are sent.
- Attachments come from files you name by path. A forward carries the original's attachments
  along, and they count in the total.
- Only regular files can be attached, and they are read when the message goes out. One that was
  moved or deleted in the meantime fails the send
  (`Could not send: could not read an attachment of this message`) and the review opens again.

## Your address and signature

- `From:` is the account's address, with `from_name` before it when set.
- A `signature` file is added under `-- ` to every new message, reply and forward.

Both are described under [`[[accounts]]`](configuration.md#accounts).
