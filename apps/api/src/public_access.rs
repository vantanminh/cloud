use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicAccessState {
    pub enabled: bool,
    pub subdomain: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicAccessUpdate {
    pub enabled: bool,
    pub subdomain: Option<String>,
    pub assigned_new_subdomain: bool,
}

pub fn public_hostname(
    enabled: bool,
    subdomain: Option<&str>,
    root_domain: Option<&str>,
) -> Option<String> {
    if !enabled {
        return None;
    }
    let subdomain = subdomain.map(str::trim).filter(|value| !value.is_empty())?;
    let domain = root_domain
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .trim_end_matches('.');
    Some(format!("{subdomain}.{domain}"))
}

pub fn should_proxy_public_host(state: &PublicAccessState, requested_subdomain: &str) -> bool {
    state.enabled
        && state
            .subdomain
            .as_deref()
            .is_some_and(|assigned| assigned == requested_subdomain)
}

pub fn enable_public_access(current: PublicAccessState) -> PublicAccessUpdate {
    if current.enabled {
        return PublicAccessUpdate {
            enabled: true,
            subdomain: current.subdomain,
            assigned_new_subdomain: false,
        };
    }
    match current.subdomain.filter(|value| !value.is_empty()) {
        Some(subdomain) => PublicAccessUpdate {
            enabled: true,
            subdomain: Some(subdomain),
            assigned_new_subdomain: false,
        },
        None => PublicAccessUpdate {
            enabled: true,
            subdomain: Some(new_public_subdomain()),
            assigned_new_subdomain: true,
        },
    }
}

pub fn disable_public_access(current: PublicAccessState) -> PublicAccessUpdate {
    PublicAccessUpdate {
        enabled: false,
        subdomain: current.subdomain,
        assigned_new_subdomain: false,
    }
}

pub fn new_public_subdomain() -> String {
    let random = Uuid::new_v4().simple().to_string();
    format!("app-{}", &random[..16])
}

pub fn is_valid_public_subdomain(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_hostname_is_absent_until_access_is_enabled() {
        assert_eq!(
            public_hostname(false, Some("app-0123456789abcdef"), Some("knotree.org")),
            None
        );
        assert_eq!(
            public_hostname(true, None, Some("knotree.org")),
            None
        );
        assert_eq!(
            public_hostname(true, Some("app-0123456789abcdef"), Some("knotree.org"))
                .as_deref(),
            Some("app-0123456789abcdef.knotree.org")
        );
    }

    #[test]
    fn enabling_assigns_a_random_subdomain_once() {
        let first = enable_public_access(PublicAccessState {
            enabled: false,
            subdomain: None,
        });
        assert!(first.enabled);
        assert!(first.assigned_new_subdomain);
        let subdomain = first.subdomain.clone().unwrap();
        assert!(subdomain.starts_with("app-"));
        assert!(is_valid_public_subdomain(&subdomain));

        let again = enable_public_access(PublicAccessState {
            enabled: false,
            subdomain: Some(subdomain.clone()),
        });
        assert_eq!(again.subdomain.as_deref(), Some(subdomain.as_str()));
        assert!(!again.assigned_new_subdomain);
    }

    #[test]
    fn disabled_and_unknown_hosts_are_not_proxied() {
        let assigned = PublicAccessState {
            enabled: true,
            subdomain: Some("app-0123456789abcdef".to_owned()),
        };
        assert!(should_proxy_public_host(&assigned, "app-0123456789abcdef"));
        assert!(!should_proxy_public_host(&assigned, "app-unknown"));
        let disabled = disable_public_access(assigned);
        assert!(!disabled.enabled);
        assert!(!should_proxy_public_host(
            &PublicAccessState {
                enabled: disabled.enabled,
                subdomain: disabled.subdomain,
            },
            "app-0123456789abcdef"
        ));
    }
}
