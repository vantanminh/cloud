# Knotree Registry integration for App services

## Status and scope

This document is the implementation plan for connecting Knotree Cloud App
services to private images in Knotree Registry. It records a cross-repository
contract and is not evidence that the integration has been implemented.

The inspected Cloud source was commit `9a6207d` (`main`). The inspected
Registry worktree was `master` at `6892ce5`, five commits ahead of
`origin/master`; its signed image webhook work must be present in the deployed
Registry before Cloud can consume it. Confirm the deployed Registry release
before beginning the webhook slice.

The Harness work is tracked by intake `IN-041` and stories `US-043` through
`US-045`.

## Product goal

From an App service, a Cloud user can choose **Connect Knotree Registry**, grant
pull-only access to an image repository, deploy a tag, and enable automatic
redeployment when that tag receives a new image. Registry push events cause
Cloud to deploy the exact digest published by the event. Registry credentials
remain server-side and never enter the user's container.

The initial scope is images hosted by `registry.knotree.com`. The connection
does not grant push, delete, or administrator access. Cloud does not build or
publish images as part of this feature.

## Verified current state

### Knotree Cloud

- App service sources are constrained to `public`, `github`, `html`, and
  `html_github`. Image source validation and create/update behavior live in
  `apps/api/src/app_services.rs`; request and response types are in
  `apps/api/src/models.rs`; the browser union is in
  `apps/web/src/lib/types.ts`.
- `github_connections` stores an encrypted per-user GitHub package token.
  Docker deployments use that token for `ghcr.io` pulls. The auto-deployer
  checks GHCR image identity once a minute and reuses the normal deployment
  path when the image changes (`apps/api/src/app_services.rs`,
  `apps/api/src/github.rs`).
- The app service database migrations currently allow only the four sources
  above. A future migration must extend the source constraint and persist a
  Registry connection reference.
- Docker deployment has credential-aware pull helpers. Kubernetes app
  workloads currently take only the globally configured
  `APP_SERVICE_IMAGE_PULL_SECRET`; `AppWorkloadSpec` has no per-service pull
  credential. A user connection cannot safely be implemented by only calling
  `docker login` in the API process.
- `apps/api/src/lib.rs` has a public GitHub HTML push hook, but no Knotree
  Registry event receiver. `html_pages::github_push_webhook` is a useful
  example for raw-body HMAC verification; it is not the Registry signature
  format.

### Knotree Registry

- OCI pulls use the standard `/v2/` protocol. A Docker/OCI client sends Basic
  credentials to `/auth/token`; the Registry intersects requested scope with
  the credential's grants and returns a short-lived Bearer token.
- `POST /api/v1/auth/tokens` creates a user-bound PAT, reveals its secret once,
  and accepts repository/action scopes. A pull connection should be granted
  only `repository:<name>:pull`. There is no separate robot identity in the
  current UI/API implementation.
- `POST /api/v1/webhooks` is administrator-only. It creates a global event
  endpoint, takes event kinds but has no repository filter, and reveals an
  HMAC secret once. Registry sends signed `tag_updated` events containing
  `metadata.immutable_image`, `metadata.tagged_image`, repository, tag, and
  digest. Delivery is at-least-once with bounded retries.
- The Registry webhook contract, header names, signature input, and event
  envelope are documented in `knotree-registry/docs/DEPLOYMENT_NOTIFICATIONS.md`
  in the sibling checkout. Use the deployed Registry's actual contract as the
  authority if it differs from that local document.

## Recommended first release

Use a pull-only PAT created by the Registry user and entered into Cloud once.
The **Connect Knotree Registry** action should open an in-product connection
wizard, explain the required repository scope, validate the credentials, and
save them encrypted. Do not ask for or store the Registry account password.

Install one `tag_updated` webhook at the Registry platform level, pointing to
Cloud's public webhook receiver. Store the one-time webhook secret in Cloud's
secret manager as `KNOTREE_REGISTRY_WEBHOOK_SECRET`. This is a one-time
operator setup because the current Registry webhook API is global and
admin-only; a Cloud tenant user must not receive Registry administrator access.
Cloud receives Registry tag events and only schedules deployments for an
active Cloud service whose configured Registry host, repository, and tag match
the event exactly.

The platform-level webhook currently sends metadata for all tagged pushes to
Cloud. Cloud should avoid persisting unmatched events and should document this
data flow for the platform operator. If repository-level event isolation is a
release requirement, add filtered, user-authorized subscriptions to Registry
before enabling the feature for tenants. Do not simulate per-user webhook
ownership in Cloud while Registry only offers a global endpoint.

This first release uses the existing credential model, so connecting requires
the user to paste a pull-only PAT. A future no-copy flow would require a
Registry authorization-code/OAuth grant and repository-scoped webhook
subscriptions; neither contract exists today.

## User and service flow

1. The user chooses a Registry image such as
   `registry.knotree.com/team/api:production` in the App service create or edit
   flow and selects **Connect Knotree Registry**.
2. Cloud shows the exact repository scope to create in the Registry dashboard:
   `repository:team/api:pull`. The user enters their Registry username and the
   one-time-revealed PAT. Cloud never accepts a push-capable scope for a pull
   connection.
3. The Cloud API validates the Registry origin and credentials by requesting
   pull authorization for that repository and checking the selected manifest.
   The API stores the PAT encrypted and returns connection metadata only.
4. Cloud creates or updates the App service, verifies that the image's host and
   repository match the connection, pulls the image, resolves its digest, and
   completes the normal deployment path.
5. The Registry sends `tag_updated` to the platform webhook endpoint. Cloud
   validates the raw-body signature, records the delivery and any matching
   deployment jobs transactionally, and acknowledges only after the database
   commit succeeds.
6. A durable worker deploys the matching image by digest. The configured tag
   remains the user's watch target; the running workload uses the immutable
   digest selected for that deployment.
7. The Cloud service settings show connection health, configured image/tag,
   deployed digest, last accepted event/deploy, auto-deploy status, and a
   disconnect/revoke action. Disconnect prevents future pulls and deployments;
   it does not stop an already-running service.

## Cloud data model plan

Add an image source value such as `knotree_registry` through a new migration;
do not overload the `github` source. Extend the Cloud UI/API union types in the
same slice.

Add a `registry_connections` table scoped to a Cloud user. The initial schema
should contain:

| Field | Purpose |
| --- | --- |
| `id` | Connection identifier referenced by services. |
| `user_id` | Owner used for Cloud authorization and disconnect checks. |
| `registry_host` | Normalized fixed host `registry.knotree.com`. |
| `registry_username` | Login name used for the Registry token exchange. |
| `repository` | Repository allowed by the connection, e.g. `team/api`. |
| `credential_ciphertext` | AES-256-GCM ciphertext using the existing Cloud credential key. |
| `verified_at`, `revoked_at`, timestamps | Connection health and lifecycle. |

Add `registry_connection_id` to `project_app_services` for Registry-sourced
services. A connection should be reusable for multiple tags/services in the
same repository, but the database and authorization layer must prevent a
service from referencing another Cloud user's connection. If the selected PAT
has access to multiple repositories, keep the first release's connection
record bound to the one repository being verified; broader grants are not
needed for this feature.

Add durable webhook delivery/deployment-job records. Store a unique Registry
delivery ID and the minimal matched event data needed to retry work. Avoid
retaining full unmatched webhook bodies. Jobs need a stable state transition
(`pending`, `running`, `succeeded`, `failed`) and a unique relation to the
delivery/service pair so duplicate Registry retries cannot create duplicate
deployments.

Use Cloud's existing `DATABASE_CREDENTIALS_ENCRYPTION_KEY` and
`security::encrypt_secret`/`decrypt_secret` boundary. Never return ciphertext or
plaintext in list APIs, app service responses, deployment logs, error messages,
or environment variables injected into tenant containers.

## Cloud API plan

Keep cookie-authenticated, CSRF-protected management routes under `/api/v1`.
Use project/workspace authorization before attaching a connection to a service.
Suggested route shapes (final names may follow existing conventions):

| Route | Behavior |
| --- | --- |
| `POST /workspaces/{workspace}/projects/{project}/registry-connections` | Validate username, PAT, fixed Registry host, and repository; encrypt and create connection. Return only connection metadata. |
| `GET /workspaces/{workspace}/projects/{project}/registry-connections` | List non-secret metadata for connections visible to this project/user. |
| `DELETE /workspaces/{workspace}/projects/{project}/registry-connections/{id}` | Revoke the Cloud connection and stop future automatic deploys that depend on it. |
| `POST /api/v1/public/webhooks/knotree-registry` | Receive Registry delivery with no browser cookie; authenticate it with the configured HMAC secret. |

The App service create/update API must accept `imageSource: "knotree_registry"`
and a connection ID. Reject the image if the fixed host, repository, tag, or
connection owner does not match. Auto-deploy settings must accept this source
only when a verified connection exists and the platform webhook receiver is
configured.

The public webhook handler must cap request size, retain the exact raw body
until signature validation completes, and return generic errors. Do not log
the webhook secret, PAT, Authorization header, or signed body.

## Private pull implementation

Define one pull-auth abstraction shared by image sources, for example
`ImagePullCredentials` with Registry host, username, and decrypted password.
Keep GitHub OAuth details inside the GitHub connector and translate them at
the deployment boundary.

For Docker deployments:

- Avoid relying on a process-global `docker login` state. Use an isolated,
  short-lived Docker config for a Registry pull/deploy job, with filesystem
  permissions restricted to the API process; remove it when the job ends.
- Pull the configured tag, record the resolved SHA-256 digest, then deploy the
  digest reference. Do not pass PAT values to `docker run`, a workload
  environment, or logs.

For Kubernetes deployments:

- Extend `AppWorkloadSpec` to carry a per-service pull-secret reference. Create
  a `kubernetes.io/dockerconfigjson` Secret for the specific App service and
  attach it through that pod's `imagePullSecrets`. Do not reuse the global
  `APP_SERVICE_IMAGE_PULL_SECRET` for tenant credentials.
- The existing app manifest uses `imagePullPolicy: IfNotPresent`; set the
  workload image to the resolved digest so a mutable local tag cannot select a
  different image. Preserve the configured tag separately for watching.
- Rotate/update the Kubernetes Secret when the Registry credential changes and
  remove it when the connection or App service is deleted. Ensure the app pod
  never receives Kubernetes API credentials; the current manifest disables
  service-account token mounting and should keep doing so.

The normal App service deploy path, progress logs, runtime limits, routing,
attached database variables, and failure behavior remain the deployment
surface. A failed pull or readiness check must leave the currently running
container/workload untouched where the existing rollout mechanism permits.

## Signed event handling and deployment queue

The Registry sends a JSON event with `schema_version: 1`. The Cloud receiver
must:

1. Require `X-Knotree-Event`, `X-Knotree-Delivery`, `X-Knotree-Timestamp`, and
   `X-Knotree-Signature`; parse the raw request body only after preserving its
   bytes.
2. Verify `X-Knotree-Signature` using HMAC-SHA256 over
   `timestamp.delivery_id.raw_body`, encoded as `sha256=<hex>`. Compare the
   MAC in constant time and reject timestamps more than five minutes from the
   Cloud clock.
3. Accept only `tag_updated` for the first release. Ignore
   `manifest_pushed`; it can describe a digest that is not assigned to the
   configured tag.
4. Match the event's `metadata.registry`, repository, tag, and tagged image to
   active Registry-sourced services. Require `metadata.is_tag == true`, a
   valid `sha256:<64 lowercase hex>` digest, and the exact configured host and
   repository. Never deploy a URL supplied by the webhook.
5. Insert the delivery ID and matching jobs in one database transaction. Use a
   uniqueness constraint for the delivery ID (and delivery/service pair).
   Return 2xx for duplicates and unmatched events. Return 2xx for a new event
   only after its work is durable; return 5xx on a transient database failure
   so Registry retries it.
6. Have a durable worker process jobs and recover pending work after an API
   restart. If several tag updates arrive while a service is deploying, keep
   the newest desired digest queued rather than acknowledging and dropping it.
7. Before rollout, compare against the currently deployed digest and skip a
   no-op. Pull/deploy by the immutable `metadata.immutable_image` reference (or
   resolve the configured tag again if a later event superseded the queued
   target), then store the digest with normal deployment status/logs.

The Registry contract is at-least-once, not exactly-once. Its current worker
retries transient failures a bounded number of times, so durable acceptance in
Cloud is required. A low-frequency tag reconciliation check can be considered
later as protection from permanently exhausted deliveries; it is not a reason
to deploy mutable tags.

## Registry operator setup

After the Cloud receiver is deployed and reachable over HTTPS, an administrator
creates one Registry webhook and stores the returned secret in Cloud's secret
manager. For example, using the Registry browser session:

```bash
curl -sS -X POST "$REGISTRY_URL/api/v1/webhooks" \
  -H "Cookie: $REGISTRY_SESSION" \
  -H 'Content-Type: application/json' \
  -d '{"url":"https://<cloud-api-origin>/api/v1/public/webhooks/knotree-registry","events":["tag_updated"]}'
```

Copy the one-time `secret` into Cloud as `KNOTREE_REGISTRY_WEBHOOK_SECRET`; do
not commit it or put it in a tenant project setting. Use the Registry admin UI
to disable/recreate the endpoint during rotation. Cloud production must fail
closed for webhook-driven auto-deploy when this secret is absent.

This registration is a platform operation, not a user action. The Cloud
**Connect Knotree Registry** button must not ask for a Registry admin session or
secret.

## UI plan

- Add Knotree Registry as a source in the App service create/edit flow.
- Provide **Connect Knotree Registry** in the account Integrations view and a
  contextual CTA in the App service form when no matching connection exists.
- Explain how to create a PAT with `repository:<repo>:pull`; collect username
  and PAT in a password-style field. Clear the field after successful save.
- Show the normalized image reference and verify status; do not show any part
  of the PAT except an optional non-secret prefix.
- In service Settings, show the connection, watched tag, deployed digest,
  automatic-deploy toggle, last Registry event/deployment, and safe error
  details. Disconnect/revoke should clearly explain that future private pulls
  and auto-deploy stop while a running service remains online.

## Harness implementation sequence

1. **US-043 — connection and private pulls.** Add the migration and scoped
   credential API, encryption, source validation, and per-provider pull support
   in both Docker and Kubernetes paths. Confirm a private image can be created,
   manually redeployed, rotated, and disconnected.
2. **US-044 — signed events and durable deploy jobs.** Add the public receiver,
   HMAC/replay/idempotency checks, event-to-service matching, persistent job
   worker, digest-pinned deployment, and recovery/concurrency behavior. Confirm
   the production Registry release delivers the documented headers and event
   shape.
3. **US-045 — UI and lifecycle.** Add the connect wizard, image source,
   Settings state, auto-deploy control, and revoke/disconnect behavior. Show a
   useful unconfigured-webhook state rather than silently promising automatic
   deploys.
4. Configure the platform webhook and secret through production secret
   management; verify the Cloud API ingress accepts Registry callbacks.
5. Run a staging end-to-end flow: create a pull-only PAT, connect a test repo,
   deploy `:staging`, push a new tag digest, observe one accepted delivery and
   one digest-pinned deployment, then replay the same delivery and observe no
   duplicate deployment.

## Planned verification

- Rust unit coverage for Registry origin/image parsing, repository scope
  matching, secret encryption/decryption, and credential ownership.
- API integration coverage for token validation, CSRF/tenant authorization,
  connection revocation, Docker pull failure, and Kubernetes pull Secret
  creation/update/deletion.
- Webhook handler coverage for valid HMAC, wrong secret, malformed headers,
  stale/future timestamp, body tampering, duplicate delivery, unmatched tag,
  invalid digest, and database failure before acknowledgement.
- Deployment coverage for no-op digest, successful digest rollout, failed pull
  preserving the existing service, multiple events during provisioning, and
  pending-job recovery after restart.
- Browser coverage for connect, invalid credential, repository mismatch,
  initial private deployment, auto-deploy toggle, webhook-not-configured state,
  disconnect, and mobile layout.
- Platform smoke: push a new image to a staging repository on
  `registry.knotree.com`; confirm the Registry emits `tag_updated`, Cloud
  returns 2xx after durable acceptance, and the App workload runs the published
  digest.

## Explicitly deferred

- Registry OAuth/authorization-code flow so users do not paste a PAT.
- Per-user or per-repository webhook subscriptions and repository-filtered
  Registry event delivery. The first release relies on one administrator
  configured platform webhook.
- Supporting arbitrary registry hosts or using Cloud as an image publisher.
- Automatically stopping a service when a connection is revoked.
