# Doing things to mail

Archiving, trashing, starring, marking read, labels and moving, selections, undo, how actions
reach the server, Microsoft 365's folders, and snooze.

- [The basic actions](#the-basic-actions)
- [Labels and moving](#labels-and-moving)
- [Selecting threads](#selecting-threads)
- [Bulk actions](#bulk-actions)
- [Undo](#undo)
- [How actions reach the server](#how-actions-reach-the-server)
- [Microsoft 365](#microsoft-365)
- [Snooze](#snooze)

The keys below are the defaults; see [Keys](keys.md) to change them.

## The basic actions

From the thread list or the reader:

| Key | Action |
|---|---|
| `e` | Archive. |
| `d` | Move to the account's trash. Rail asks first (see below). |
| `s` | Star or unstar. It stars unless every thread is already starred. |
| `u` | Mark read or unread. It marks read if any thread is unread. |

An action applies to the whole thread, as in Gmail.

`u` toggles. To have one key that only marks read and another that only marks unread, bind
`list.mark_read` and `list.mark_unread` in `[keys]` (they have no key by default):

```toml
[keys]
"list.mark_read" = "M"
"list.mark_unread" = "N"
```

### Trash

`d` asks first, `Trash "subject"?` or `Trash 3 threads?`, and says what the trash does:

- Gmail keeps trashed mail in its Trash for 30 days;
- on Microsoft 365 it `Moves to Deleted Items.`

Rail never deletes mail for good.

### When Rail asks first

- Over 50 threads (`bulk_confirm` under `[general]`), `e`, `s`, `u`, `list.mark_read` and
  `list.mark_unread` ask first too, naming the count, for example `Archive 1,234 threads?`.
- `d` always asks.
- Trash without asking (`list.trash_now`, `D` in the vim and mutt presets) asks only over that count.

### What happens next

The list changes at once. The daemon sends the change to the account's server (Gmail or
Microsoft 365) in the background, so it also goes through if you quit Rail.

Opening an unread thread marks it read at once in the reader; the list row and the unread counts
follow a moment later. `u` marks it unread again.

## Labels and moving

`l` opens the account's labels:

| Key | Action |
|---|---|
| Letters | Filter. |
| `Down` / `Up` (or `Ctrl-n` / `Ctrl-p`) | Move, because letters filter. |
| `Space` | Tick or untick. |
| `Enter` | Apply: labels you ticked are added, labels you unticked are removed. |
| `Esc` | Cancel. |

For a selection, the ticks start from every label any selected thread carries.

On Gmail, `m` opens the same list to move: `Enter` adds the highlighted label and archives the
thread.

- Gmail's system folders (Inbox, Sent, Trash and the like) are not in the list.
- Rail does not create labels.
- A selection that spans two accounts cannot be labelled or moved at once; Rail says
  `Labels belong to one account: select threads from one account.`

On Microsoft 365, see [Microsoft 365](#microsoft-365).

## Selecting threads

### One at a time

`Space` selects the thread under the cursor (a `▸` replaces its dot, `>` with `icons = "ascii"`)
and moves down. The header counts the selection (`3 selected`).

- With a selection, an action applies to every selected thread, then the selection clears.
- `Esc` clears it.
- The selection stays while you change the filter or the mailbox.
- In the reader, the keys act on the open thread, not on the selection, which stays as it was
  (`Space` still pages down, and selecting is for the list). Archive, trash and move close the
  reader.

### Everything that matches: `* a`

`* a` selects every thread this mailbox and filter match, newest first, at most 10,000. The header
then reads `10,000 selected (more match)`. When nothing matches it says `Nothing matches.` and
selects nothing.

### Everything from a sender: `* f`

`* f` puts the sender of the thread under the cursor into the filter line as `from:<address>`, and
selects every thread with any message from that address, as the same `from:` filter would show
them.

- In the filter line that form matches that address exactly, where a plain `from:dana` matches
  every thread whose sender's name *or* address contains `dana`.
- The sender is that of the thread's newest message from someone else. Your own messages (from
  the account's address, or in its Sent or Drafts) are passed over, so a thread you answered last
  selects by the person you answered.
- A thread with no such message with a readable sender, or one whose address the filter cannot
  carry, says `This thread has no sender to select by.` and changes nothing.

If you change the mailbox or the filter before `* a` or `* f` has answered, its answer is dropped
and nothing is selected.

### A range: `v`

`v` starts a range at the thread under the cursor. The bar says `range from here`, and while the
range is open its first key hint is `Esc range`.

1. Move as usual.
2. `v` or `Esc` ends the range and keeps every thread it covered.
3. `Esc` again clears the selection, as before.

If the thread you started from has left the list by then, Rail says
`The range start is no longer in the list.` and selects nothing.

### Extending: `J` / `K`

`J` or `Shift-↓` extends the selection down from the thread under the cursor, and `K` or `Shift-↑`
up. The first press selects that thread and the next, and each further press one more.

- Going back toward where you started unselects what the extension added; threads you had
  selected before stay selected.
- Any other key ends the extension and keeps what it selected, so `J` after a move starts again
  from the new place.
- A terminal that does not report Shift with the arrows still has `J` and `K`.
- Elsewhere in Rail, the Drafts folder included, a Shift-arrow moves like the plain arrow, and
  there `J` and `K` move too.
- While `v` has a range open, `J`, `K` and the Shift-arrows only move.
- If a sync reads the list again while you extend and the thread you started from leaves it, the
  extension carries on from where the cursor was.

A selection holds at most 10,000 threads. An extension or a `v` range it stops says
`The selection holds at most 10,000 threads.`

## Bulk actions

A bulk action goes out as one request per 200 threads per account.

- One `z` takes the whole set back, reporting `undone` once.
- An undo that could take only part of it back says `undone in part: some were already sent`.
- When one of the requests fails, its threads come back to the selection and its error stays on
  the status bar while the others answer.

## Undo

After an action the status bar says what happened, in place of the key hints, for example
`archived 3 · z undoes`.

| Situation | `z` says |
|---|---|
| Within ten seconds | Takes the action back. |
| After ten seconds | `Too late to undo.` |
| Nothing done | `Nothing to undo.` |
| The daemon has not answered yet | `undoing…`, and finishes when it does. |

- Only the most recent action can be undone.
- Marking a thread read by opening it has no undo.
- `z` works anywhere in the mail screen.
- An earlier action whose answer comes after a newer one still shows its error if it failed (its
  selected threads come back to the selection), and is still undone if `z` was pressed for it.
  That undo says only `too late to undo` when it came too late.

## How actions reach the server

Each account in the status bar shows actions on their way to its server (`2 queued`) and actions
the server refused (`1 failed`).

### Order and targets

- The daemon sends the actions on one thread in the order you made them.
- An action waiting to be tried again holds back only the later actions on its own threads.
- Queued moves, label changes and undo keep targeting the original folder in the original account
  across syncs. If that folder has been deleted, Rail does not substitute another folder.

### Retries

- When Gmail cannot be reached it tries again after 1, then 5, then 15 minutes, then every 15
  minutes, and gives up on an action after 20 attempts.
- While an account needs a login, its actions wait and are sent after the login.
- An action still waiting when Rail starts again is sent.
- If recording a server result in the local store fails, that account's queue waits while the
  daemon retries the store write after one second, doubling the wait up to one minute. It keeps
  the server result and does not repeat the provider action; the queue continues once the write
  succeeds.

### Refusals

When Gmail refuses an action for good (the thread or the label no longer exists), Rail:

1. undoes it in the list;
2. shows "Could not archive: …" on the thread's row;
3. keeps the error in `:errors`, where `Enter` on it sends the action again.

Refusals are listed in `:errors` again whenever Rail connects to the daemon, so one that happened
while Rail was closed or disconnected is not lost.

A later sync that brings Gmail's state of that thread clears the refusal and its `1 failed` (a
snoozed thread's failed return excepted: see [Snooze](#when-a-return-fails)).

## Microsoft 365

On a Microsoft 365 account the same keys act through Outlook's folders, where a message lives in
one folder.

| Key | On Microsoft 365 |
|---|---|
| `e` | Moves the thread's Inbox messages to Archive. On a mailbox with no Archive folder it says `This mailbox has no Archive folder; press m to move.` (Rail never makes one). |
| `d` | Moves the thread's messages to Deleted Items (the prompt says `Moves to Deleted Items.`). |
| `m` | Lists Inbox, Archive, Junk Email and your own folders, and moves the thread's messages to the one you pick. |
| `s` | Flags the newest message, or clears every flag of a flagged thread. |
| `u` | Marks every unread message read, or the newest message unread. |
| `l` | Answers `Outlook uses folders, not labels: press m to move.` |

- Archive, trash and move leave your own messages in Sent Items and Drafts where they are.
- A selection may hold threads of both kinds: each account gets its own change, and `l` is refused
  for the whole selection.
- `m` is refused for a selection spanning accounts:
  `Labels belong to one account: select threads from one account.`
- When Outlook refuses some messages of a thread (one deleted meanwhile, say), the others still
  change and the refusals are logged. Only when it refuses every message does the action fail.
- Opening an unread thread marks it read in Outlook (the status bar shows `1 queued` meanwhile).

More on Microsoft 365 accounts: [Microsoft 365 mail](accounts.md#microsoft-365-mail).

## Snooze

`h` snoozes the thread under the cursor, the selection or the open thread (which closes). Rail
archives it now and puts it back in the inbox at the time you choose, unread, first under TODAY.

### The chooser

| Choice | Time | When it is offered |
|---|---|---|
| `Later today` | 18:00 | Before 17:00 |
| `Tomorrow` | 08:00 | |
| `This weekend` | Saturday 08:00 | Monday to Friday |
| `Next week` | Monday 08:00 | |

Each is shown with its time. A choice that falls at the time of an earlier one is left out (on a
Friday, this weekend is tomorrow).

`Enter` snoozes, `Esc` cancels, `Down`/`Up` move.

### Typing a time

There is no calendar to click through: type a time instead, in numbers or in words.

| Form | Examples |
|---|---|
| A time of day (today, or tomorrow once it has passed) | `18:00`, `6pm`, `6:30 pm`, `noon` |
| A span | `3 days`, `in 2 hours`, `an hour`, `90 min`, `1 week`, `1 month`, `+3h` |
| A day | `tomorrow`, `tonight`, `later today`, `this weekend`, `next week`, `fri`, `next monday` |
| A date (without a year, a date that has passed this year means next year's) | `nov 3`, `3 nov 2026`, `november 3rd`, `2026-11-03` |
| A day with a time, in either order | `tomorrow 9am`, `9am fri`, `nov 3 at 14:00` |

- 24-hour and am/pm times both work whatever `time` is set to.
- A day without a time comes back at 08:00, `tonight` at 20:00 and `later today` at 18:00.
- What you type is offered first as `At` with its time. Typing a choice's name (`tomorrow`) picks
  that choice.

| You type | Rail says |
|---|---|
| Text Rail cannot read | `Try 6pm, tomorrow 9am, 2 days, nov 3 or fri.` |
| A time that has passed | `That time has passed.` |
| Less than about a minute ahead | `That is too soon.` |
| More than a year ahead | `That is more than a year away.` |

`Enter` checks the time against the clock once more, so one that became too soon while the
chooser was open is refused there and nothing is sent.

### Last used

The last time you typed is offered first next time as `Last used`, read again against the clock
(`2 days` counts from when you open the chooser), unless a choice already offers that time.

Rail keeps it in the state directory (`snooze-last`), so it survives a restart.

### After snoozing

The status bar says `Snoozed until Mon 08:00 · z undoes`, and for ten seconds `z` takes the snooze
back, as with archive.

- On a thread that is already snoozed the chooser's title says until when. Choosing a time moves
  the snooze, and its first row, `Return to inbox now`, puts the thread back at once
  (`returned 1 to the inbox`).
- While anything is snoozed, `Snoozed` is listed under UNIFIED in the sidebar with the number of
  snoozed threads (`◷`, or `^` with `icons = "ascii"`). `:` and `Mailbox: Snoozed` open it at any
  time, and an empty one says `Nothing is snoozed.`
- A thread whose snoozed mail was all trashed, moved or put back in the inbox since has nothing
  left to bring back, so it is neither listed nor offered `Return to inbox now`.
- The reader shows `Snoozed until Mon 13 Oct 2025 08:00` under a snoozed thread's recipients.
- Drafts cannot be snoozed (`Not available on drafts`).

### Snoozing a selection

A selection is snoozed as one.

- The chooser's title counts it (`Snooze 3 threads until`, with `Return to inbox now` first when
  some of it is snoozed).
- The snooze takes the threads that have mail in the inbox, or are snoozed already, and leaves the
  others where they are. The status bar counts both
  (`Snoozed 118 until Mon 08:00 · z undoes · 2 not in the inbox`).
- Only a snooze with none to take is refused (`Only a thread in the inbox can be snoozed.`).
- `Return to inbox now` counts what came back as its requests answer
  (`returned 250 to the inbox`), and a request that fails says why and gives its threads back to
  the selection.

### Coming back

- New mail in the thread brings it back early.
- Your own reply, which lands in Sent, does not.
- Neither does a message of the snooze put back in the inbox from another device (snoozing the
  thread again then archives it with the rest).

When threads come back the status bar says `Back from snooze: <subject>` or
`3 threads back from snooze`.

### How snooze works on the server

Neither Gmail nor Microsoft 365 offers a snooze Rail can use, so Rail keeps the time itself. Other
devices show the thread archived until it comes back, and it comes back while the daemon runs
(see [The background daemon](configuration.md#the-background-daemon)).

The return puts back exactly the messages the snooze took out of the inbox:

- on Gmail it adds `INBOX` to them (one you trashed or reported as spam meanwhile stays there);
- on Microsoft 365 it moves them from Archive back to Inbox (one you moved to another folder
  meanwhile stays there).

If the server refuses the archive, the snooze fails with it (`Could not archive: …`, and `Enter`
in `:errors` sends it again). Snoozing the thread again starts afresh.

### When a return fails

A return the server cannot take right away is tried again like any action. One it refuses, or that
gives up after 20 attempts, leaves the thread archived and stays in `:errors`
(`Could not return to the inbox: …`) through later syncs, where `Enter` sends it again. A retry
leaves mail you trashed or reported as spam since where it is.

It leaves `:errors`:

- once it is sent;
- once the thread is back in the inbox by other means;
- once none of its messages is still archived (all trashed, reported or filed elsewhere);
- or when you archive, trash or move the thread in Rail.

So a return the server keeps refusing stays until you send it again with `Enter` in `:errors` and
it goes through, or you archive, trash or move the thread in Rail to give it up.

Removing an account leaves its snoozed threads archived, since nothing syncs that account any more.
