# Troubleshooting

Common problems and messages, and what to do about each, grouped by area. Microsoft's own sign-in
error codes are at the end.

- [Start here](#start-here)
- [Logins and tokens](#logins-and-tokens)
- [The daemon](#the-daemon)
- [Sync](#sync)
- [Microsoft 365](#microsoft-365)
- [A folder cannot be synced](#a-folder-cannot-be-synced)
- [Drafts and writing](#drafts-and-writing)
- [Display](#display)
- [Microsoft sign-in errors](#microsoft-sign-in-errors)

## Start here

Start with `rail doctor`.

Every error Rail prints is followed by a line starting `hint:` that says what to do next. Words
from outside Rail (a provider's or the system's) are quoted on a line of their own.

## Logins and tokens

| Symptom | Fix |
|---|---|
| A token refresh fails with "refresh token rejected", or `rail doctor` shows tokens `missing` | `rail account login <name>`. Common after a revoked grant, a sign-in frequency policy, or tokens deleted from the keyring. |
| `the signed-in mailbox does not match this account's address` | Log in again and select the account's mailbox. For Google Workspace, configure its primary mailbox address rather than a custom alias. Existing credentials are kept. |
| Gmail asks you to log in again every week | The OAuth consent screen is still in "Testing"; publish it ("In production"). |
| No Secret Service (tty-only Linux, containers) | Set `secrets = "file"` under `[general]` before adding accounts, or move existing tokens with `rail secrets set file`. |
| "1Password is locked" or `op` not signed in | Unlock the app, or turn on its CLI integration; `rail doctor` checks with `op whoami`. |
| The login URL did not open in a browser | Select it on screen and open it while Rail is still running. |

## The daemon

| Symptom | Fix |
|---|---|
| `no operation named "…"` | The daemon is from an earlier build. Rail now replaces an older build by itself; if you still see this, run `rail daemon stop`. |
| `rail doctor` fails because the daemon cannot start, e.g. the socket path is too long | Set `RAIL_RUNTIME_DIR` to a shorter directory. |
| "a Rail daemon is already running with different directories" | Run `rail daemon stop` from the shell that started it, or use the same `RAIL_*` settings. |
| "Rail was upgraded; restart this program" | An older `rail mcp` or wizard is still open after an upgrade; restart it. After a rebuild, if restarting does not help, this `rail` is the older build: run `rail daemon stop` (or use the newer binary). |
| `rail daemon install` says no user service manager answers | Some containers have no `systemctl --user`; the daemon still starts on demand. |
| `rail doctor` warns that the login unit does not match this binary | You moved or reinstalled `rail` elsewhere; run `rail daemon install` again. |

More on the daemon: [The background daemon](configuration.md#the-background-daemon).

## Sync

| Symptom | Fix |
|---|---|
| A first sync is taking hours | Expected for a large mailbox; set `history_days` to sync only recent mail. `rail doctor` and `rail_sync_status` show progress. |

See [Sync settings](configuration.md#sync-settings).

## Microsoft 365

| Symptom | Fix |
|---|---|
| `Microsoft 365 refused access: an administrator may need to grant consent (see "Work or school accounts and admin consent" in docs/accounts.md)` | An administrator grants consent, as in [Work or school accounts and admin consent](accounts.md#work-or-school-accounts-and-admin-consent), then run `rail account login <name>` if the login also fails. |
| `this mailbox cannot be reached through Microsoft Graph (on-premises, unlicensed or inactive)` | The mailbox must be an Exchange Online mailbox with a licence. |
| `Outlook uses folders, not labels: press m to move.` | Expected: Outlook has folders, not labels. Press `m` to move the thread to a folder. |
| `This mailbox has no Archive folder; press m to move.` | The mailbox has no Archive folder and Rail never makes one. Press `m` to move the thread to a folder. |
| `This mailbox has no Deleted Items folder.` | `d` found no Deleted Items folder in the last folder listing, which an Exchange Online mailbox always has; Rail put the thread back. Press `Ctrl-r` to sync, or `m` to move the thread to a folder. |
| `Could not …: Rail holds no messages of this Microsoft 365 conversation` | The thread's messages left the store before the action was sent (another device moved or deleted them). Press `Ctrl-r`; the next sync shows where they are now. |
| `a folder cannot be synced: its sync link is longer than 4 KiB` | See [A folder cannot be synced](#a-folder-cannot-be-synced). |
| `the folder for this move no longer exists in Microsoft 365` | The folder was deleted in Outlook before the move was sent; Rail put the thread back. Press `m` and pick another folder. |
| `Microsoft 365 is busy or unavailable (HTTP 429)`, or `retrying` in `rail sync` | Microsoft is throttling the mailbox. Rail waits as long as Microsoft asks (up to two minutes at a time) and tries again. |
| A reply from Microsoft 365 is not in the Outlook conversation | The message it answered was gone from Outlook for good (Deleted Items emptied) before the send, so Rail sent the reply as a new message. |

## A folder cannot be synced

The message is `a folder cannot be synced: its sync link is longer than 4 KiB`. It shows in the
header as `⚠ name sync failed`, and `:errors` names the folder after it, as
`(from outside Rail (the provider or the system): "Projects")`.

**Why.** Microsoft answered three times in a row with a sync link for that folder longer than Rail
keeps.

**What happens:**

- Rail stops syncing that folder until the daemon restarts (`rail daemon stop`). Every other
  folder goes on, and the mail Rail holds from it stays.
- Until the restart, or until the folder is deleted in Outlook, every pass counts as failed:
  `rail doctor`'s last-success time stops advancing and its sync check fails, and `rail sync`
  exits 1.
- Mail deleted on the server while the folder is given up on stays in Rail. A full resync (when
  Microsoft expires a sync link, a synced folder is deleted, or `history_days` grows) that runs
  while the folder is still given up on removes nothing, and a restart alone does not remove it
  either. It goes with the first full resync after a restart.

Please report it if it comes back.

## Drafts and writing

| Symptom | Fix |
|---|---|
| `Gmail no longer has this draft` | The draft was deleted in Gmail since Rail last synced, and its row goes after the next sync. A draft you already opened stays on this computer, and `s` saves it to Gmail again. |
| `Microsoft 365 no longer has this draft` | The draft was deleted in Outlook since Rail last synced, and its row goes after the next sync. A draft you already opened stays on this computer, and `s` saves it to Outlook again. |
| `draft: saving to Gmail, try again in a moment` | A save of this draft to Gmail is still on its way, so it cannot be discarded yet. Wait for `saving to Gmail…` to leave the status bar and press `d` again. |
| `draft: saving to Microsoft 365, try again in a moment` | A save of this draft to Outlook's Drafts is still on its way, so it cannot be discarded yet. Wait for `saving to Microsoft 365…` to leave the status bar and press `d` again. |
| Two copies of one draft in Gmail's Drafts | Creating a Gmail draft is tried once; a failure that hid a success can leave a second copy. Delete the extra one in Gmail. |
| The editor opens and Rail comes straight back with the message unchanged | A graphical editor that returns at once. Give it its wait flag in `general.editor`, e.g. `editor = "code --wait"`. |
| `cannot run "vi"` (or your editor's name) when writing | No editor was found: `general.editor`, `$VISUAL` and `$EDITOR` are unset or name a program that is not installed, and `vi` is missing. Set `general.editor`. |
| The editor's text is still on screen after it closes | Rail could not take the terminal back from the editor. It repaints the whole screen on the next frame and logs `could not take the terminal back from the editor` in `rail.log`. |
| `Could not send: Gmail refused the request (HTTP 400)` | Gmail refused the message and did not say why. The review opens again: check the addresses with `t` and send again. |

See [Writing mail](writing.md).

## Display

| Symptom | Fix |
|---|---|
| Colours look wrong or a theme does nothing inside tmux | Add `set -as terminal-features ",xterm-256color:RGB"` (tmux 3.2+) or `set -ga terminal-overrides ",*:Tc"` to `~/.tmux.conf`, and make sure `COLORTERM=truecolor` reaches the shell inside tmux. |

See [Themes](reading.md#themes).

## Microsoft sign-in errors

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

See [Work or school accounts and admin consent](accounts.md#work-or-school-accounts-and-admin-consent).
