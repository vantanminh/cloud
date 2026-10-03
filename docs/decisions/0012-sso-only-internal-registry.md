---
created_at: "2026-10-03T15:39:48.203722500+00:00"
doc: docs/decisions/0012-sso-only-internal-registry.md
id: "0012"
links:
  - US-049
  - US-050
  - "0011"
notes: "Supersedes DEC-0011. Cloud removes password register/login; /login and /register lead to Knotree Accounts (screen_hint=signup for sign-up). Users are keyed by Knotree sub only and email/name/username are refreshed on each sign-in; users.email is no longer unique. Password-only users are not migrated; their sessions are revoked. Cloud calls Registry's internal API with a projected ServiceAccount token (audience knotree-registry-internal) naming the signed-in account; Registry serves only that account's namespace and issues per-repository pull-only credentials that Cloud renews (30 days before expiry, before deploys and every 6 hours). Consent, manual token connections and the Integrations connect/disconnect flow are removed; the retired account tables are dropped in a later release."
status: accepted
title: SSO-only sign-in and internal Registry trust
type: decision
updated_at: "2026-10-03T15:39:48.203727400+00:00"
verify: null
---

# SSO-only sign-in and internal Registry trust
