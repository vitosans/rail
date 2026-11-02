//! Reading the receiving server's `Authentication-Results`: which From domains it vouched for,
//! and whether a sender's address is one of them.

use super::auth_results::{aligned, vouched_domains};

const GMAIL: Option<&str> = Some("mx.google.com");

#[test]
fn gmail_s_dmarc_pass_vouches_for_the_from_domain() {
    let header = "mx.google.com;\r\n       dkim=pass header.i=@atlassian.net header.s=s1 \
                  header.b=abc;\r\n       spf=pass (google.com: domain of bounce@atlassian.net \
                  designates 1.2.3.4 as permitted sender) smtp.mailfrom=bounce@atlassian.net;\r\n       \
                  dmarc=pass (p=REJECT sp=REJECT dis=NONE) header.from=Company.Atlassian.net";
    assert_eq!(
        vouched_domains(header, GMAIL),
        ["atlassian.net", "company.atlassian.net"]
    );
}

#[test]
fn microsoft_s_results_carry_no_server_name_and_a_domain_is_listed_once() {
    let header = "spf=pass (sender IP is 1.2.3.4) smtp.mailfrom=example.com; dkim=pass \
                  (signature was verified) header.d=example.com;dmarc=pass action=none \
                  header.from=example.com;compauth=pass reason=100";
    assert_eq!(vouched_domains(header, None), ["example.com"]);
}

#[test]
fn a_result_that_did_not_pass_vouches_for_nothing() {
    let header = "mx.google.com; dkim=fail header.d=example.com; spf=pass \
                  smtp.mailfrom=example.com; dmarc=fail (p=NONE) header.from=example.com";
    assert!(vouched_domains(header, GMAIL).is_empty());
}

#[test]
fn spf_alone_vouches_for_nothing_since_it_checks_the_envelope_not_the_from_line() {
    let header = "mx.google.com; spf=pass smtp.mailfrom=example.com";
    assert!(vouched_domains(header, GMAIL).is_empty());
}

#[test]
fn results_from_another_server_are_not_gmail_s() {
    let header = "evil.example; dmarc=pass header.from=example.com";
    assert!(vouched_domains(header, GMAIL).is_empty());
    let unnamed = "dmarc=pass header.from=example.com";
    assert!(vouched_domains(unnamed, GMAIL).is_empty());
}

#[test]
fn a_property_inside_a_comment_is_no_property() {
    let header = "mx.google.com; dmarc=fail (header.from=example.com dmarc=pass) \
                  header.from=example.org; dkim=pass (a (nested) header.d=example.com) \
                  header.d=\"example.net\"";
    assert_eq!(vouched_domains(header, GMAIL), ["example.net"]);
}

#[test]
fn a_method_with_a_version_and_odd_case_is_read() {
    let header = "MX.Google.Com 1; DKIM/1=PASS Header.D=Example.COM";
    assert_eq!(vouched_domains(header, GMAIL), ["example.com"]);
}

#[test]
fn a_sender_aligns_with_its_own_domain_or_a_parent_of_it() {
    let vouched = ["company.atlassian.net".to_string()];
    assert!(aligned("Jira@Company.Atlassian.Net", &vouched));
    assert!(aligned("jira@eu.company.atlassian.net", &vouched));
    assert!(!aligned("jira@atlassian.net", &vouched));
    assert!(!aligned("jira@evilcompany.atlassian.net", &vouched));
    assert!(!aligned("jira@company.atlassian.net.evil", &vouched));
    assert!(!aligned("no-at-sign", &vouched));
    assert!(!aligned("jira@company.atlassian.net", &[]));
}
