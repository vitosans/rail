//! Every action a key can trigger: its stable id, the views it works in, its default keys and
//! what `?` says about it. The ids never change once released: `[keys]` in `config.toml`
//! names them.

/// Where a key is read. A view's own bindings come first; `Global` ones work in every mail
/// view whose own bindings do not take the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum View {
    Global,
    List,
    Sidebar,
    Reader,
    Compose,
    Wizard,
}

impl View {
    pub const ALL: [View; 6] = [
        View::Global,
        View::List,
        View::Sidebar,
        View::Reader,
        View::Compose,
        View::Wizard,
    ];

    /// The name in messages: "the list view".
    pub fn name(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::List => "list",
            Self::Sidebar => "sidebar",
            Self::Reader => "reader",
            Self::Compose => "compose",
            Self::Wizard => "wizard",
        }
    }

    /// The heading in help.
    pub fn title(self) -> &'static str {
        match self {
            Self::Global => "Everywhere",
            Self::List => "Thread list",
            Self::Sidebar => "Sidebar",
            Self::Reader => "Reader",
            Self::Compose => "Composer",
            Self::Wizard => "Setup wizard",
        }
    }
}

macro_rules! actions {
    ($( $variant:ident => $id:literal, [$($view:ident),+], [$($key:literal),*], $desc:literal; )+) => {
        /// One thing a key can do.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Action {
            $($variant),+
        }

        impl Action {
            /// Every action, in the order help lists them.
            pub const ALL: &'static [Action] = &[$(Action::$variant),+];

            /// The id `[keys]` uses.
            pub fn id(self) -> &'static str {
                match self {
                    $(Self::$variant => $id),+
                }
            }

            /// The views the action works in.
            pub fn views(self) -> &'static [View] {
                match self {
                    $(Self::$variant => &[$(View::$view),+]),+
                }
            }

            /// The default key names (see `parse_binding`).
            pub fn default_keys(self) -> &'static [&'static str] {
                match self {
                    $(Self::$variant => &[$($key),*]),+
                }
            }

            /// What help says the action does.
            pub fn description(self) -> &'static str {
                match self {
                    $(Self::$variant => $desc),+
                }
            }
        }
    };
}

actions! {
    GlobalQuit => "global.quit", [Global], ["q"], "Quit Rail";
    GlobalPalette => "global.palette", [Global], [":"], "Open the command palette";
    GlobalHelp => "global.help", [Global], ["?"], "Show the keys for this view";
    GlobalUndo => "global.undo", [Global], ["z"], "Undo the last action";
    ListOpen => "list.open", [List], ["enter"], "Open the thread";
    ListFilter => "list.filter", [List], ["/"], "Filter the list as you type";
    ListSaveSearch => "list.save_search", [List], ["S"], "Save the filter as a search";
    ListSidebar => "list.sidebar", [List, Sidebar], ["b"], "Show or hide the sidebar";
    ListAccount1 => "list.account_1", [List, Sidebar], ["1"], "Go to account 1's inbox";
    ListAccount2 => "list.account_2", [List, Sidebar], ["2"], "Go to account 2's inbox";
    ListAccount3 => "list.account_3", [List, Sidebar], ["3"], "Go to account 3's inbox";
    ListAccount4 => "list.account_4", [List, Sidebar], ["4"], "Go to account 4's inbox";
    ListAccount5 => "list.account_5", [List, Sidebar], ["5"], "Go to account 5's inbox";
    ListAccount6 => "list.account_6", [List, Sidebar], ["6"], "Go to account 6's inbox";
    ListAccount7 => "list.account_7", [List, Sidebar], ["7"], "Go to account 7's inbox";
    ListAccount8 => "list.account_8", [List, Sidebar], ["8"], "Go to account 8's inbox";
    ListAccount9 => "list.account_9", [List, Sidebar], ["9"], "Go to account 9's inbox";
    ListUnified => "list.unified", [List, Sidebar], ["0"], "Go to the unified inbox";
    ListRefresh => "list.refresh", [List, Sidebar, Reader], ["ctrl-r"], "Sync now; reconnect to the daemon";
    ListDown => "list.down", [List], ["j", "down"], "Next thread";
    ListUp => "list.up", [List], ["k", "up"], "Previous thread";
    ListPageDown => "list.page_down", [List], ["pagedown", "ctrl-d"], "One page down";
    ListPageUp => "list.page_up", [List], ["pageup", "ctrl-u"], "One page up";
    ListTop => "list.top", [List], ["home", "g g"], "Newest thread";
    ListBottom => "list.bottom", [List], ["end", "G"], "Oldest thread";
    ListArchive => "list.archive", [List, Reader], ["e"], "Archive the thread or the selection";
    ListTrash => "list.trash", [List, Reader], ["d"], "Move to the trash (asks first)";
    ListStar => "list.star", [List, Reader], ["s"], "Star or unstar";
    ListRead => "list.read", [List, Reader], ["u"], "Mark read or unread";
    ListLabels => "list.labels", [List, Reader], ["l"], "Add or remove labels (Gmail)";
    ListMove => "list.move", [List, Reader], ["m"], "Move to a label (Gmail) or a folder (Microsoft 365)";
    ListSnooze => "list.snooze", [List, Reader], ["h"], "Snooze until later, or return a snoozed thread";
    ListCompose => "list.compose", [List, Reader], ["c"], "Write a new message";
    ListOpenLink => "list.open_link", [List, Reader], ["g t"], "Open the link the subject names";
    ListReply => "list.reply", [List, Reader], ["r"], "Reply to the latest message";
    ListReplyAll => "list.reply_all", [List, Reader], ["R"], "Reply to everyone";
    ListForward => "list.forward", [List, Reader], ["f"], "Forward the latest message";
    ListSelect => "list.select", [List], ["space"], "Select or unselect the thread";
    ListClearSelection => "list.clear_selection", [List], ["esc"], "Clear the selection";
    ListSelectAll => "list.select_all", [List], ["* a"], "Select every thread this mailbox and filter match";
    ListSelectSender => "list.select_sender", [List], ["* f"], "Select every thread from this sender";
    ListSelectRange => "list.select_range", [List], ["v"], "Select a range of threads";
    ListExtendDown => "list.extend_down", [List], ["J", "shift-down"], "Extend the selection down";
    ListExtendUp => "list.extend_up", [List], ["K", "shift-up"], "Extend the selection up";
    SidebarFocus => "sidebar.focus", [List, Sidebar], ["tab"], "Move between the sidebar and the list";
    SidebarDown => "sidebar.down", [Sidebar], ["j", "down"], "Next mailbox";
    SidebarUp => "sidebar.up", [Sidebar], ["k", "up"], "Previous mailbox";
    SidebarPageDown => "sidebar.page_down", [Sidebar], ["pagedown", "ctrl-d"], "One page down";
    SidebarPageUp => "sidebar.page_up", [Sidebar], ["pageup", "ctrl-u"], "One page up";
    SidebarOpen => "sidebar.open", [Sidebar], ["enter"], "Show this mailbox";
    SidebarFold => "sidebar.fold", [Sidebar], ["space", "left", "right"], "Fold or unfold this account or folder";
    SidebarRenameSearch => "sidebar.rename_search", [Sidebar], ["r"], "Rename this saved search";
    SidebarRemoveSearch => "sidebar.remove_search", [Sidebar], ["d"], "Remove this saved search (asks first)";
    ReaderNext => "reader.next", [Reader], ["n"], "Next message";
    ReaderPrev => "reader.prev", [Reader], ["p"], "Previous message";
    ReaderExpand => "reader.expand", [Reader], ["tab"], "Expand or collapse the message";
    ReaderUnfold => "reader.unfold", [Reader], ["enter"], "Show the quoted lines, or open the message";
    ReaderHeaders => "reader.headers", [Reader], ["H"], "Show or hide the headers";
    ReaderOpenBrowser => "reader.open_browser", [Reader], ["o"], "Open the message in the browser";
    ReaderAttachments => "reader.attachments", [Reader], ["a"], "List the attachments";
    ReaderSaveAttachments => "reader.save_attachments", [Reader], ["w"], "Save this message's attachments";
    ReaderLoadImages => "reader.load_images", [Reader], ["i"], "Load this thread's remote pictures";
    ReaderTrustImages => "reader.trust_images", [Reader], ["I"], "Always load this sender's remote pictures, or stop";
    ReaderFollowLink => "reader.follow_link", [Reader], ["g x"], "Open a link";
    ReaderBack => "reader.back", [Reader], ["q", "esc"], "Close the thread";
    ReaderDown => "reader.down", [Reader], ["j", "down"], "Next line";
    ReaderUp => "reader.up", [Reader], ["k", "up"], "Previous line";
    ReaderPageDown => "reader.page_down", [Reader], ["pagedown", "space"], "One page down";
    ReaderPageUp => "reader.page_up", [Reader], ["pageup"], "One page up";
    ReaderTop => "reader.top", [Reader], ["home", "g g"], "First line";
    ReaderBottom => "reader.bottom", [Reader], ["end", "G"], "Last line";
    ComposeSend => "compose.send", [Compose], ["y"], "Send";
    ComposePush => "compose.push", [Compose], ["s"], "Save to Drafts (Gmail or Microsoft 365)";
    ComposeEdit => "compose.edit", [Compose], ["e"], "Edit in your editor";
    ComposeHeaders => "compose.headers", [Compose], ["t"], "Change To, Cc, Bcc or Subject";
    ComposeAttach => "compose.attach", [Compose], ["a"], "Attach a file";
    ComposeRemove => "compose.remove", [Compose], ["x"], "Remove the highlighted attachment";
    ComposeClose => "compose.close", [Compose], ["q", "esc"], "Close and keep the draft";
    ComposeDiscard => "compose.discard", [Compose], ["d"], "Discard the draft (asks first)";
    ComposeDown => "compose.down", [Compose], ["j", "down"], "Next line";
    ComposeUp => "compose.up", [Compose], ["k", "up"], "Previous line";
    ComposePageDown => "compose.page_down", [Compose], ["pagedown", "space"], "One page down";
    ComposePageUp => "compose.page_up", [Compose], ["pageup"], "One page up";
    WizardBack => "wizard.back", [Wizard], ["esc"], "Go back one step";
    WizardPortal => "wizard.portal", [Wizard], ["f1"], "Open the provider portal or the preset's help";
    WizardNextField => "wizard.next_field", [Wizard], ["tab"], "Next field";
    WizardPrevField => "wizard.prev_field", [Wizard], ["backtab"], "Previous field";
    WizardNext => "wizard.next", [Wizard], ["enter"], "Choose or continue";
    WizardUp => "wizard.up", [Wizard], ["up", "k"], "Move up";
    WizardDown => "wizard.down", [Wizard], ["down", "j"], "Move down";
    WizardPageUp => "wizard.page_up", [Wizard], ["pageup"], "Scroll one page up";
    WizardPageDown => "wizard.page_down", [Wizard], ["pagedown"], "Scroll one page down";
    WizardTop => "wizard.top", [Wizard], ["home"], "Scroll to the top";
    WizardBottom => "wizard.bottom", [Wizard], ["end"], "Scroll to the end";
    WizardOpenPortal => "wizard.open_portal", [Wizard], ["o"], "Open the portal from the instructions";
    WizardToggle => "wizard.toggle", [Wizard], ["space"], "Tick or untick the checkbox";
    WizardRetry => "wizard.retry", [Wizard], ["r"], "Retry after an error";
    WizardCopyUrl => "wizard.copy_url", [Wizard], ["c"], "Copy the URL shown";
}

impl Action {
    /// The action with this id.
    pub fn from_id(id: &str) -> Option<Action> {
        Self::ALL.iter().copied().find(|a| a.id() == id)
    }
}
