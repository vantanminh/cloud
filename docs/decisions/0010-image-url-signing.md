---
created_at: "2026-10-02T08:41:16.786247618+00:00"
doc: docs/decisions/0010-image-url-signing.md
id: DEC-0010
links:
  - IN-046
  - US-047
notes: "Public image URLs are HMAC-signed with a platform key derived from the credentials encryption key. Revoking a client id and secret disables upload, delete, listing, and private URLs. Already signed public URLs keep verifying. Private URLs also bind key id and expiry. Compression is a store setting: none, fixed WebP, or per-URL WebP."
status: accepted
title: Sign image URLs with a platform key independent of client credentials
type: decision
updated_at: "2026-10-02T08:41:16.786248083+00:00"
verify: Public serve does not consult key status. Private serve rejects revoked keys and expired signatures.
---

# Sign image URLs with a platform key independent of client credentials
