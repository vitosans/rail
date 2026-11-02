//! User configuration: `config.toml`.

pub mod edit;
pub mod link;
pub mod load;
pub mod lock;
#[cfg(test)]
mod lock_tests;
pub mod model;
pub mod saved;

pub use edit::{
    append_account, default_account_name, put_saved_search, remove_account, remove_saved_search,
    set_general, set_general_bool, set_general_number, set_general_text, set_secrets,
};
pub use link::{
    Compiled as CompiledLinkAction, LinkAction, MAX_LINK_ACTION_NAME_CHARS, MAX_LINK_ACTIONS,
    MAX_MATCHES, Match as LinkMatch, choices as link_choices, compile_all as compile_link_actions,
};
pub use load::{ConfigError, MAX_ADDRESS_CHARS, line_col, load, load_text_or_empty, parse};
pub use model::{
    AccentSpec, AccountConfig, AccountKind, AiSend, Config, DAY_SECS, DimRead, FolderModel,
    General, Icons, KeymapPreset, Layout, OnePasswordConfig, SecretsBackend, SyncSettings,
    ThreadOrder, TimeFormat, is_format_char, is_plain, not_plain,
};
pub use saved::{
    MAX_SAVED_SEARCH_NAME_CHARS, MAX_SAVED_SEARCHES, NameProblem, SavedSearch, check_name,
    check_put, duplicate_name, name_rule,
};
/// A config value with its byte range in the file (for line numbers).
pub use toml::Spanned;
