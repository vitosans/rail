# Account setup

How to register an OAuth app with Google or Microsoft, add the account to Rail, and what a
Microsoft 365 account does differently from Gmail.

- [Before you start](#before-you-start)
- [The setup wizard](#the-setup-wizard)
- [Gmail / Google Workspace](#gmail--google-workspace)
- [Microsoft 365 / Outlook](#microsoft-365--outlook)
- [Work or school accounts and admin consent](#work-or-school-accounts-and-admin-consent)
- [Microsoft 365 mail](#microsoft-365-mail)
- [Presets for your organisation](#presets-for-your-organisation)

## Before you start

Rail ships no OAuth client of its own. You register one per provider once (or use a preset from
your organisation), then log in. The wizard walks you through the same steps shown here.

An AI agent connected over MCP can also do this for you; see [MCP](mcp.md).

## The setup wizard

`rail setup` opens the setup wizard. Running `rail` with no config does the same.

1. Choose a preset your organisation gave you, or set up your own app.
2. For your own app, the wizard shows the portal steps from this page and opens the portal
   with `o`.
3. It asks for the values the portal gave you.
4. It logs you in through the browser.

The wizard needs an interactive terminal. In scripts, use
[`rail account add` with flags](commands.md).

### Wizard keys

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

These are the wizard's default keys, the `wizard.*` actions in the
[action table](keys.md#every-action); each can be changed.

### How the wizard exits

| Situation | Result |
|---|---|
| At least one account was saved | Exits 0. |
| No account was saved | Exits 1 with `setup cancelled; nothing was saved`. |
| You quit while the browser login is pending | That login is cancelled. |
| You quit while an account's save had already started | The save is always finished, never left half-written; Rail says so and exits 0. |
| You quit after an admin-consent error | Rail prints the admin-consent URL once the terminal is back, so you can forward it to an administrator. |
| stdout is not a terminal | Every way of opening the wizard prints a hint and exits 2. |

### Checking the signed-in mailbox

After a browser login, Rail verifies that the signed-in mailbox matches the account's address
before saving credentials or adding the account. Selecting a different mailbox fails the login
and preserves existing credentials.

- Address comparisons ignore case.
- **Gmail:** use the primary mailbox address. Consumer Gmail dot, plus-tag and `googlemail.com`
  variants also match. Google Workspace custom aliases require the primary address.
- **Microsoft 365:** also accepts the signed-in user's principal name and SMTP aliases returned
  by Graph.

Verification uses the existing OAuth permissions. If the profile cannot be read, finish the login
again once the error is resolved.

## Gmail / Google Workspace

1. In Google Cloud Console create a project and enable the **Gmail API**.
2. **OAuth consent screen**: External. **Publishing status must be "In production"**, even
   unverified. An app left in "Testing" has its refresh tokens expire after 7 days, and Rail will
   keep asking you to log in.
3. **Credentials**: create an **OAuth client ID** of type **Desktop app** and copy the client id
   and secret. A desktop-app secret is not actually secret; keeping it in `config.toml` is fine
   (or keep it in [1Password](configuration.md#tokens-in-1password)).
4. Log in with `rail setup`, or:

   ```sh
   rail account add gmail --address you@example.com \
     --client-id XXXX.apps.googleusercontent.com --client-secret GOCSPX-...
   ```

   A browser opens for consent. Google warns that your own app is unverified; continue.

<!-- guide:google v1 -->
### The same steps with `gcloud`

`gcloud` can create the project and enable the API, but it cannot create a desktop OAuth client,
so steps 2 and 3 stay in the browser.

Pick a project id: 6 to 30 characters, lowercase letters, digits and hyphens, starting with a
letter and not ending with a hyphen.

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
[Using Rail from an AI agent](mcp.md)).

## Microsoft 365 / Outlook

1. In Azure Portal, **App registrations** > New.
   - Supported account types: pick what matches your mailbox (work/school, personal, or both).
   - Redirect URI: platform **Mobile and desktop applications**, value `http://127.0.0.1`. Rail
     listens on `http://127.0.0.1:<random port>`, and Azure matches any port for a loopback
     redirect.
   - If the portal refuses an `http://127.0.0.1` URI, add it by editing the app's **Manifest**
     (`replyUrlsWithType`) instead.
2. Under **Authentication**, set "Allow public client flows" to **Yes**. Rail is a desktop app and
   holds no client secret; without this Microsoft answers `AADSTS7000218`.
3. **API permissions**: Microsoft Graph delegated `Mail.ReadWrite`, `Mail.Send`, `User.Read`,
   `offline_access`.
4. Log in with `rail setup`, or:

   ```sh
   rail account add m365 --address you@example.com --client-id <application-id>
   ```

   The client ID is the **Application (client) ID** on the registration's **Overview** page, not
   the *Object ID*.

   For a single-tenant app ("Accounts in this organizational directory only") also pass
   `--tenant <tenant-id-or-domain>` (or fill in the wizard's *Tenant* field): Microsoft refuses
   the default `common` endpoint for single-tenant apps. Add `--tenant` too if your organisation
   restricts `common`.

<!-- guide:microsoft v1 -->
### The same steps with `az`

The Azure CLI can do the whole registration of an app for work or school accounts (single or
multiple organisations). Personal Microsoft accounts use the portal steps above.

| Placeholder | Value |
|---|---|
| `<tenant>` | The tenant id or a domain the tenant owns. |
| `<audience>` | `AzureADMyOrg` (one organisation) or `AzureADMultipleOrgs`. |

```sh
az version
az login --tenant <tenant>
az account show --query tenantId --output tsv
az ad app create --display-name Rail --sign-in-audience <audience> --public-client-redirect-uris http://127.0.0.1 --is-fallback-public-client true --query appId --output tsv
az ad sp show --id 00000003-0000-0000-c000-000000000000 --query "oauth2PermissionScopes[?value=='Mail.ReadWrite' || value=='Mail.Send' || value=='User.Read' || value=='offline_access'].{value:value, id:id}" --output table
az ad app permission add --id <app-id> --api 00000003-0000-0000-c000-000000000000 --api-permissions <mail-readwrite-id>=Scope <mail-send-id>=Scope <user-read-id>=Scope <offline-access-id>=Scope
```

- `az ad app create` prints the Application (client) ID; use it as `<app-id>` and as
  `--client-id`.
- `az ad sp show` lists the ids of the four Microsoft Graph permissions for
  `az ad app permission add`.

An administrator can then approve Rail for the whole organisation:

```sh
az ad sp create --id <app-id>
az ad app permission admin-consent --id <app-id>
```

Approval needs the app's identity in the tenant. An "already exists" error from the first command
means it is done. If the second command reports that the permissions or the service principal are
not found, wait a minute and run it again, because directory changes take a moment to propagate.

## Work or school accounts and admin consent

### Single or multi-tenant

- "Accounts in this organizational directory only" (single tenant) is right for one
  organisation; log in through its tenant ID or domain.
- Choose "Accounts in any organizational directory" only when several organisations share one
  registration. Then `common` works, but each organisation still has to consent.

### Finding the tenant ID

In Azure Portal open **Microsoft Entra ID** > **Overview** and copy the *Tenant ID*, or copy the
*Directory (tenant) ID* from your app registration's **Overview**.

A domain the tenant owns (`contoso.onmicrosoft.com`, or a verified domain such as `contoso.com`)
works in the same places.

### Admin consent

Many organisations stop users from approving apps themselves. An administrator approves Rail once
for everyone, in any of three ways:

| Where | How |
|---|---|
| From the wizard | On the Details step tick **I am an administrator of this tenant**. Rail opens Microsoft's admin-consent page for your tenant, waits for the redirect to `http://127.0.0.1`, and confirms, then continues with your own login. The tenant must be a concrete tenant ID or domain, not `common`; the wizard asks for it if needed. |
| From the portal | Open the app's **API permissions** and choose **Grant admin consent for &lt;organisation&gt;**. |
| During a normal login | An administrator who logs in is shown Microsoft's consent page with a **Consent on behalf of your organization** checkbox; ticking it grants consent for everyone. |

If you are not an administrator and your login fails with `AADSTS65001` or `AADSTS90094`, the
wizard shows the exact admin-consent URL:

1. Press `c` to copy it.
2. Send it to an administrator.
3. Press `r` to retry once they have approved.

Tenant policies (Conditional Access, app assignment, MFA) can still intervene after consent; see
[Microsoft sign-in errors](troubleshooting.md#microsoft-sign-in-errors).

## Microsoft 365 mail

What a Microsoft 365 account does today. It syncs, reads, acts on threads and writes mail over
Microsoft Graph.

### At a glance

| | Gmail | Microsoft 365 |
|---|---|---|
| A thread is | a Gmail thread | an Outlook conversation |
| A star is | a Gmail star | Outlook's flag |
| `l` | Labels | Answers `Outlook uses folders, not labels: press m to move.` |
| `m` | Adds a label and archives | Moves to Inbox, Archive, Junk Email or one of your folders |
| `e` | Archives | Moves Inbox messages to Archive |
| `d` | Trash (kept 30 days) | Deleted Items |
| Search | Gmail's search syntax | `from:`, `subject:`, `to:`, `body:` and plain words; at most 1,000 results from the server |
| Drafts | Gmail's Drafts | Outlook's Drafts |
| Message size | 25 MiB, 50 files | The same |

### Folders

- Every mail folder is synced, nested ones as `Parent/Child` (a `/` in a folder's own name shows
  as `∕`).
- Inbox, Sent Items, Drafts, Archive, Deleted Items and Junk Email are the account's system
  folders in the sidebar, under those names, and fill `Sent` and `Trash` under UNIFIED.
- Every other folder is under `More folders`, nested ones as a tree.
- Search folders and Conversation History, RSS Feeds, RSS Subscriptions and Sync Issues are left
  out. Those last four are matched by their English names, so a mailbox in another language may
  show them.

### Sync

- **The first sync** shows `⟳` and a running count with no total, because Microsoft gives no
  cheap one. It reads folder by folder, the inbox first. New mail then arrives within one
  `poll_interval`.
- **History.** With `history_days` set, Microsoft returns at most 5,000 messages per folder; mail
  in a folder beyond that is not synced. Raising `history_days` reads every folder again.
- **Full reads.** When Microsoft forgets a folder's place (after a long time offline), or a folder
  is deleted, the account reads every folder again and removes what is gone. The exception: while
  a folder's subfolders cannot be listed or a folder is given up on, nothing is removed until a
  later full read (see `a folder cannot be synced` in [Troubleshooting](troubleshooting.md)).

### Search

`from:`, `subject:`, `to:`, `body:` and plain words work; Gmail's `label:`, `is:` and `has:` do
not. A search returns at most 1,000 results from the server.

### Reading and attachments

- A OneDrive or SharePoint link attachment is listed. When Rail is asked for its bytes it says
  `This attachment is a link (OneDrive or SharePoint); open the message in Outlook on the web to reach it.`
- A message whose attachments are all inline lists none.
- When Microsoft refuses a message's attachment list, Rail keeps the list it already had rather
  than losing it, and does not ask again.
- For mail stored before 0.4.0 that kept list can be out of date: it may show an inline picture
  or an attached message as an ordinary file. Forwarding that message, or attaching it as the
  original, may then try to send that part as a file.
- Microsoft rarely refuses the same list for good, and opening a picture that is missing fetches
  the message's body again, which corrects the list whenever Microsoft gives it. If a forward of
  such a message goes wrong, forward it from Outlook on the web.
- **Marking read.** Opening an unread thread marks its unread messages read in Outlook; `u` marks
  them read, or the newest message unread.

### Acting on threads

- `e` archives to Outlook's Archive folder. A mailbox with no Archive folder says
  `This mailbox has no Archive folder; press m to move.`
- `d` moves to Deleted Items.
- `m` moves to Inbox, Archive, Junk Email or one of your folders.
- `s` flags the newest message (or clears every flag).
- Each has `z` to undo. Your own messages in Sent Items and Drafts stay where they are.
- **Folders, not labels.** `l` answers `Outlook uses folders, not labels: press m to move.`
  Outlook categories are not used.

See [Doing things to mail](actions.md#microsoft-365) for the details.

### Writing

- `c`, `r`, `R` and `f` write as on Gmail, and `y` sends with the ten second `z` undo.
- Rail writes each message as an Outlook draft. A reply, reply to everyone or forward is made on
  the original message, so it joins the Outlook conversation and others see it threaded. Rail
  then sets its subject, text, recipients and `Message-ID`.
- The text goes as plain text. `y` sends that draft, and the sent copy arrives in Sent Items with
  the next sync.
- An original still in Deleted Items keeps the threading. A reply or forward whose original is
  gone from Outlook for good (Deleted Items emptied) goes out as a new message, outside the
  conversation.

### Drafts

- `s` saves the draft to Outlook's Drafts (`saved to Microsoft 365 · z undoes`). Later saves
  update the same Outlook draft, and one deleted in Outlook is made again in Drafts (the deleted
  copy stays in Deleted Items).
- Sending a saved draft sends that very draft, so none is left in Drafts.
- Opening a draft written in Outlook fetches its current body and attachment listing, even when
  Rail has cached them; a failed fetch stops the import. Once imported, the local draft keeps your
  edits when reopened.
- `d` on it deletes it in Outlook (it goes to Deleted Items).
- **An extra copy can stay in Outlook's Drafts:**
  - when Outlook made a reply, reply to everyone or forward draft but its answer was lost on the
    way back (a timeout, say) or could not be read, Rail makes another on the retry;
  - when a save made the Outlook draft but then failed for good (a permanent attachment error,
    say), discarding the draft in Rail leaves that copy.

  Delete the extra one in Outlook's Drafts.

### Attachments when writing

- At most 50 files and 25 MiB in all, as on Gmail
  (`the message is … MiB, larger than Microsoft 365's 25 MiB limit`).
- A file of 3 MiB or more goes up in 3.75 MiB pieces, and a failed upload is tried again later.
- A forward keeps the original's attachments in Outlook without downloading them; remove one in
  the review and it goes from the draft too.
- Rail matches its attachments to the draft's by name and the size Outlook lists for them, so a
  save or send uploads only a file that is new or has changed since (other bytes, or the same
  length written again).
- Rail writes the message as plain text, so pictures inside the body of a reply, a forward or an
  imported Outlook draft are dropped from it.
- Attached messages and OneDrive or SharePoint links stay as Outlook has them: Rail does not list
  them for removal and never downloads or uploads them.

### Never sent twice

Before any later attempt (a retry, or a start after the daemon stopped mid-send), Rail asks
Outlook for a sent message with the draft's `Message-ID`, and about the draft it saved, and counts
a sent one as sent.

## Presets for your organisation

An administrator can save every colleague the portal work:

1. Register the app once (as above, with public client flows enabled and admin consent granted).
2. Hand out a preset file that fills in the wizard.

A preset never contains tokens, and Rail never writes preset files.

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

### Publishing a preset

| Where | Notes |
|---|---|
| A file on each machine | `/etc/rail/presets/*.toml`, or a user's `~/.config/rail/presets/*.toml` (`$RAIL_CONFIG_DIR/presets` when that is set). The wizard lists these. |
| The intranet, over `https` | People type the URL on the preset step. Rail refuses plain `http://`, redirects (an intranet URL behind single sign-on fails with "HTTP 302"; use the file instead) and bodies over 64 KiB, and never saves the fetched file. |

A colleague then runs `rail setup`, chooses **Use a preset from my organisation**, picks the preset
(or types its path or URL), enters their address and logs in.
