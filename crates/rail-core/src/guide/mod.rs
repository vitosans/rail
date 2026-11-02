//! Provider setup as data: the steps an agent walks a user through to register an OAuth app.
//! Rail returns commands, pages and tool calls; it never runs them. The setup sections of
//! docs/accounts.md carry the same commands and pages, and a test keeps the two in step.

mod google;
mod microsoft;

use crate::config::AccountKind;
use crate::ops::{
    Audience, GuideInputs, GuideRequest, GuideResponse, GuideStep, OpError, StepKind,
};

/// Bumped whenever a step changes. docs/accounts.md marks the version it documents.
pub const GUIDE_VERSION: u32 = 1;

/// The steps for `request.kind`, with the inputs filled in. Anything not given stays as a
/// `<placeholder>` for the agent to fill from an earlier step's output or by asking.
///
/// Inputs end up inside shell commands the agent runs, so each is validated first; a value
/// that does not look like what it names is refused without being echoed.
pub fn guide(request: &GuideRequest) -> Result<GuideResponse, OpError> {
    let swaps = replacements(&request.inputs)?;
    let steps = template(request.kind, &request.inputs)
        .into_iter()
        .map(|step| fill(step, &swaps))
        .collect();
    Ok(GuideResponse {
        version: GUIDE_VERSION,
        kind: request.kind,
        steps,
    })
}

/// The steps as written, placeholders and all.
pub fn template(kind: AccountKind, inputs: &GuideInputs) -> Vec<GuideStep> {
    match kind {
        AccountKind::Gmail => google::steps(),
        AccountKind::M365 => microsoft::steps(inputs.admin),
    }
}

/// The `<placeholder>`s `guide` fills, and with what, after validating each value.
fn replacements(inputs: &GuideInputs) -> Result<Vec<(&'static str, String)>, OpError> {
    let mut out = Vec::new();
    if let Some(project) = &inputs.project_id {
        if !valid_project_id(project) {
            return Err(OpError::invalid_input(
                "project_id is not a valid Google Cloud project id",
            )
            .with_hint(
                "6 to 30 characters: a lowercase letter, then lowercase letters, digits or hyphens, not ending in a hyphen",
            ));
        }
        out.push(("<project-id>", project.clone()));
    }
    if let Some(tenant) = &inputs.tenant {
        if !valid_tenant(tenant) {
            return Err(OpError::invalid_input(
                "tenant is neither a tenant id (a GUID) nor a domain name",
            )
            .with_hint(
                "use the tenant id or a domain the tenant owns, such as contoso.onmicrosoft.com",
            ));
        }
        out.push(("<tenant>", tenant.clone()));
    }
    if let Some(audience) = inputs.audience {
        let value = match audience {
            Audience::SingleTenant => "AzureADMyOrg",
            Audience::MultiTenant => "AzureADMultipleOrgs",
        };
        out.push(("<audience>", value.to_string()));
    }
    Ok(out)
}

/// `^[a-z][a-z0-9-]{4,28}[a-z0-9]$`.
fn valid_project_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (6..=30).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1] != b'-'
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// A tenant GUID, or a DNS name of `[A-Za-z0-9-]` labels joined by dots.
fn valid_tenant(tenant: &str) -> bool {
    is_guid(tenant) || is_dns_name(tenant)
}

fn is_guid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, len)| g.len() == len && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn is_dns_name(text: &str) -> bool {
    text.len() <= 253
        && text.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

fn fill(step: GuideStep, swaps: &[(&'static str, String)]) -> GuideStep {
    let apply = |mut text: String| {
        for (from, to) in swaps {
            text = text.replace(from, to);
        }
        text
    };
    GuideStep {
        command: step.command.map(apply),
        url: step.url.map(apply),
        tool: step.tool.map(apply),
        check: step.check.map(apply),
        content: step.content.map(apply),
        ..step
    }
}

/// A step with only the parts that apply set.
pub(crate) fn step(kind: StepKind, title: &str, explanation: &str) -> GuideStep {
    GuideStep {
        kind,
        title: title.to_string(),
        explanation: explanation.to_string(),
        command: None,
        url: None,
        ask: None,
        tool: None,
        check: None,
        content: None,
        optional: false,
    }
}

#[cfg(test)]
mod tests;
