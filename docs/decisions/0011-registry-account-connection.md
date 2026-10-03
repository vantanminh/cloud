---
created_at: "2026-10-03T12:18:44.900689800+00:00"
doc: docs/decisions/0011-registry-account-connection.md
id: DEC-0011
links:
  - IN-047
  - US-048
notes: "One SSO-bound consent gives Cloud a pull-only credential for the user's whole Registry namespace. Project connections derive from the account row and always use its live credential. tag_updated deploys an account-derived connection only when the event's owner_issuer and owner_subject match the account. Registry sends grant_revoked when the owner revokes the credential; Cloud revokes the account or legacy connection and turns off auto-deploy. The picker only ever uses the signed-in user's own credential."
status: accepted
title: Connect Knotree Registry once per account with owner-verified auto-deploy
type: decision
updated_at: "2026-10-03T12:18:44.900693700+00:00"
verify: Webhook test rejects mismatched owners; revocation DB test only revokes the reported owner; dialog test imports from the connected account without a token.
---

# Connect Knotree Registry once per account with owner-verified auto-deploy
