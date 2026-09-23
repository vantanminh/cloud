# Knotree Registry integration for App services

## Status and scope

The Cloud-side implementation is in place in this repository. It adds a
project-scoped Registry connection, private Docker/Kubernetes pulls, immutable
digest deployments, and a signed webhook-to-durable-job auto-deploy path. The
Harness work is tracked by intake `IN-041` and stories `US-043` through
`US-045`.

This is not a production release record. The API migration must be applied,
the platform webhook secret must be provisioned, and a staging push-to-deploy
smoke test must pass before enabling this for production users. The Registry
must be running the signed `tag_updated` webhook contract documented in
`knotree-registry/docs/DEPLOYMENT_NOTIFICATIONS.md`.

## Product goal

From an App service, a Cloud user can choose **Connect Knotree Registry**, grant
pull-only access to an image repository, deploy a tag, and enable automatic
redeployment when that tag receives a new image. Registry push events cause
Cloud to deploy the exact digest published by the event. Registry credentials
remain server-side and never enter the user's container.

The initial scope is images hosted by `registry.knotree.com`. Cloud uses the
credential only to request pull-scoped access and never builds or publishes
images. Users must create a pull-only PAT: Cloud cannot inspect or reduce
broader grants already attached to an opaque PAT.

## Verified current state

### Knotree Cloud

- App services accept the `knotree_registry` image source. Image source
  validation, digest deployment, and durable job processing live in
  `apps/api/src/app_services.rs`; connection and webhook endpoints live in
  `apps/api/src/knotree_registry.rs`; browser types are in
  `apps/web/src/lib/types.ts`.
- `github_connections` stores an encrypted per-user GitHub package token.
  Docker deployments use that token for `ghcr.io` pulls. The auto-deployer
  checks GHCR image identity once a minute and reuses the normal deployment
  path when the image changes (`apps/api/src/app_services.rs`,
  `apps/api/src/github.rs`).
- Migration `0018_knotree_registry.sql` adds project-bound connection records,
  the per-service connection reference, and idempotent delivery/deploy-job
  records.
- Docker pulls use a short-lived private Docker config rather than shared
  `docker login` state. Kubernetes workloads use a per-service
  `kubernetes.io/dockerconfigjson` Secret instead of the global
  `APP_SERVICE_IMAGE_PULL_SECRET`.
- `/api/v1/public/webhooks/knotree-registry` validates signed Registry events
  and stores matching jobs before acknowledging them. The worker processes
  durable jobs, coalesces stale pending digests, and renews its lease during
  long deployments.

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

## Implemented architecture

Use a pull-only PAT created by the Registry user and entered into Cloud once.
The **Connect Knotree Registry** action opens an in-product connection flow,
explains the required repository scope, validates pull access, and saves the
PAT encrypted. Cloud does not ask for or store the Registry account password.

Install one `tag_updated` webhook at the Registry platform level, pointing to
Cloud's public webhook receiver. Store the one-time webhook secret in Cloud's
secret manager as `KNOTREE_REGISTRY_WEBHOOK_SECRET`. This is an operator setup
because the current Registry webhook API is global and admin-only; a Cloud
tenant user must not receive Registry administrator access. Cloud reports
auto-deploy readiness only when this secret is configured.
Cloud receives Registry tag events and only schedules deployments for an
active Cloud service whose configured Registry host, repository, and tag match
the event exactly.

The platform-level webhook currently sends metadata for all tagged pushes to
Cloud. Cloud stores only a minimal delivery ID/event-kind record for an
unmatched signed event and does not retain its body or repository metadata;
document this data flow for the platform operator. If repository-level event
isolation is a release requirement, add filtered, user-authorized subscriptions
to Registry before enabling the feature for tenants. Do not simulate per-user
webhook ownership in Cloud while Registry only offers a global endpoint.

Connecting requires the user to paste a PAT created with only
`repository:<repo>:pull`. Cloud requests a pull-scoped bearer token and uses
the saved PAT only for Registry token exchange. The Registry does not expose
an API here for Cloud to inspect all grants on a PAT, so the Cloud form's
pull-only instruction is important; secret storage and all pull codepaths must
continue to treat it as a pull credential. A future no-copy flow would require
a Registry authorization-code/OAuth grant and repository-scoped webhook
subscriptions; neither contract exists today.

## User and service flow

1. The user chooses a Registry image such as
   `registry.knotree.com/team/api:production` in the App service create flow
   and selects **Connect Knotree Registry**.
2. Cloud shows the exact repository scope to create in the Registry dashboard:
   `repository:team/api:pull`. The user enters their Registry username and a
   PAT created with only that pull grant. Cloud can confirm pull access, but
   the Registry API does not let Cloud inspect whether the PAT has broader
   grants, so this must be checked when the PAT is created.
3. The Cloud API validates the Registry origin and credentials by requesting
   pull authorization for that repository and checking the selected manifest.
   The API stores the PAT encrypted and returns connection metadata only.
4. Cloud creates the App service, verifies that the image's host and
   repository match the connection, resolves its digest, and completes the
   normal deployment path using the immutable digest reference.
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

## Cloud data model

Migration `0018_knotree_registry.sql` adds the `knotree_registry` source; it
does not overload `github`. Connection records are project-scoped and retain
the user who created them:

| Field | Purpose |
| --- | --- |
| `id` | Connection identifier referenced by services. |
| `user_id` | Owner used for Cloud authorization and disconnect checks. |
| `registry_host` | Normalized fixed host `registry.knotree.com`. |
| `registry_username` | Login name used for the Registry token exchange. |
| `repository` | Repository allowed by the connection, e.g. `team/api`. |
| `credential_ciphertext` | AES-256-GCM ciphertext using the existing Cloud credential key. |
| `verified_at`, `revoked_at`, timestamps | Connection health and lifecycle. |

`registry_connection_id` links a service to a connection for its exact
repository. A project connection can be reused by services in that project;
management queries verify workspace/project access, and service creation
checks repository equality. Connections are not shared across projects.

Webhook delivery IDs are unique. The API stores only the event kind and
receive time for a delivery, plus matching service jobs with a validated
digest/reference and state (`pending`, `running`, `succeeded`, `failed`). It
does not retain the raw body or unmatched payload. The unique
delivery/service relation makes Registry retries idempotent.

Use Cloud's existing `DATABASE_CREDENTIALS_ENCRYPTION_KEY` and
`security::encrypt_secret`/`decrypt_secret` boundary. Never return ciphertext or
plaintext in list APIs, app service responses, deployment logs, error messages,
or environment variables injected into tenant containers.

## Cloud API

Management routes are cookie-authenticated and CSRF-protected, and authorize
the workspace/project before reading or mutating a connection:

| Route | Behavior |
| --- | --- |
| `POST /api/v1/workspaces/{workspace}/projects/{project}/registry-connections` | Accept `{ username, token, repository }`; verify pull access to the fixed Registry host, encrypt the PAT, and return metadata only. |
| `GET /api/v1/workspaces/{workspace}/projects/{project}/registry-connections` | List non-secret metadata plus `autoDeployReady` for that project's connections. |
| `PATCH /api/v1/workspaces/{workspace}/projects/{project}/registry-connections/{id}` | Verify and rotate the PAT; update per-service Kubernetes pull secrets before replacing the encrypted credential. |
| `DELETE /api/v1/workspaces/{workspace}/projects/{project}/registry-connections/{id}` | Revoke the Cloud connection, disable dependent auto-deploy services, detach the connection, fail pending jobs, and attempt to delete per-service Kubernetes pull secrets. Cleanup errors are logged. Running services are left online. This does not revoke the PAT at Registry. |
| `POST /api/v1/public/webhooks/knotree-registry` | Receive a Registry delivery without a browser cookie; authenticate with the configured HMAC secret. Body limit is 64 KiB. |

The App service create API accepts `imageSource: "knotree_registry"` and
`registryConnectionId`. It rejects arbitrary hosts, untagged images,
repository/connection mismatches, and unverified pull access. Enabling
auto-deploy requires an active connection and configured webhook secret.

The public webhook handler caps request size at 64 KiB, retains the exact raw
body until signature validation completes, and returns generic errors. It does
not log the webhook secret, PAT, Authorization header, or signed body.

## Private pull implementation

Registry and GitHub have separate source credential types. Credentials are
loaded only at the deployment boundary; Registry PATs are never returned in
the service response or passed into the app container.

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
  remove it when the Cloud connection is disconnected. Disconnect cleanup is
  best-effort and failures are logged. Ensure the app pod never receives
  Kubernetes API credentials; the manifest disables service-account token
  mounting.

The normal App service deploy path, progress logs, runtime limits, routing,
attached database variables, and failure behavior remain the deployment
surface. A failed pull or readiness check must leave the currently running
container/workload untouched where the existing rollout mechanism permits.

## Signed event handling and deployment queue

The Registry sends a JSON event with `schema_version: 1`. The Cloud receiver:

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
   no-op. Construct the immutable reference from the validated repository and
   digest fields (rather than trusting a webhook-supplied URL), deploy by that
   reference, then store the digest with normal deployment status/logs.

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

## Cloud deployment configuration

- Migration `0018_knotree_registry.sql` is applied by the Helm
  pre-upgrade/pre-install migration Job when `migrations.enabled=true`. The
  production `deploy/k3s-pull.sh` flow enables this hook. Confirm the Job
  succeeds before relying on the new API routes.
- For local development, set `KNOTREE_REGISTRY_WEBHOOK_SECRET` in the API
  environment file after creating a test Registry webhook. It may be empty
  when testing pulls without auto-deploy. If set, it must be at least 32 bytes;
  a shorter value prevents the API from starting.
- In Kubernetes, provision that value out-of-band in the chart's existing
  Secret (default `knotree-api-secrets`) under the key configured by
  `secrets.knotreeRegistryWebhookSecretKey` (default
  `KNOTREE_REGISTRY_WEBHOOK_SECRET`). Do not put its plaintext into Helm
  values, Git, or tenant settings. When the key is absent, the API remains
  available for manual pulls but reports `autoDeployReady: false` and rejects
  attempts to enable Registry auto-deploy.
- Expose `POST /api/v1/public/webhooks/knotree-registry` through the API HTTPS
  ingress. It is intentionally unauthenticated at the browser/session layer;
  the HMAC is its authentication boundary. Ensure proxies preserve the
  `X-Knotree-*` headers and raw request body.

## UI behavior implemented

- Knotree Registry is selectable as a source in the App service create flow.
- The form loads project-scoped connections and offers **Connect Knotree
  Registry** when none matches the selected repository. It explains the exact
  required pull scope and collects username/PAT in an input that is not sent
  to the service environment.
- The create action verifies/saves the connection before creating the service;
  the API response never includes the token. Automatic deploy is disabled in
  the form when `autoDeployReady` is false.
- Settings shows the configured image/tag, connected/disconnected state,
  deployed digest, and Registry auto-deploy control. It supports token
  rotation and disconnect. Rotation requires a fresh PAT. Disconnect revokes
  only Cloud's saved connection; the PAT itself must also be revoked in the
  Registry dashboard if it should no longer be usable. After disconnect, the
  existing service remains running, but must be recreated with a new
  connection to resume Registry pulls.

## Implementation and release status

1. **US-043 — connection/private pulls:** implemented in the Cloud API and
   Docker/Kubernetes deployment paths.
2. **US-044 — signed events/durable jobs:** implemented with replay-bounded
   HMAC validation, idempotent delivery storage, a persistent worker, digest
   pinning, push coalescing, and restart recovery.
3. **US-045 — UI/lifecycle:** implemented in the service create and Settings
   views, including token rotation, auto-deploy readiness, and disconnect.
4. **Still required before release:** enable and confirm the migration Job,
   provision `KNOTREE_REGISTRY_WEBHOOK_SECRET`, register the platform-level
   Registry webhook, and verify ingress reaches this API.
5. **Still required before release:** run the staging flow below against the
   deployed Registry/Cloud versions. No production push or deployment is
   part of this code change.

## Verification

- Rust unit tests cover fixed Registry image/tag validation, digest
  references, Docker auth-config generation, and exact HMAC inputs.
- Rust unit tests cover per-service Kubernetes image pull Secret generation
  and workload wiring; frontend tests cover connecting before service
  creation.
- Verified locally: `pnpm typecheck:web`, `pnpm test:web`, `pnpm build:web`,
  `cargo check --manifest-path apps/api/Cargo.toml`,
  `cargo test --manifest-path apps/api/Cargo.toml`, `helm lint
  deploy/helm/knotree-api`, targeted ESLint, and `rustfmt --check` for the new
  `knotree_registry.rs` module.
- Workspace `pnpm lint:web` remains red on two `react-hooks/set-state-in-effect`
  violations in unchanged `apps/web/src/components/html-page-workspace.tsx`
  lines 34 and 165. Workspace-wide `cargo fmt --check` reports formatting
  diffs across existing crate modules; the new Registry module passes its
  individual formatter check.
- API/database integration, real private image pulls, webhook delivery/replay,
  and end-to-end deployment require the target services and remain staging
  release checks; they are not proven by unit tests.
- Browser-based visual QA was not available in this environment (no Browser
  plugin and no installed Playwright CLI); the create and Settings interactions
  are covered by Vitest component tests instead.

Staging smoke checklist:

1. Create a new PAT scoped only to `repository:<repo>:pull`; connect it to a
   Cloud test project and deploy a test tag.
2. Confirm the service reports the resolved SHA-256 digest, the workload uses
   the digest reference, and no Registry credential appears in service
   variables, responses, or logs.
3. Push a new digest to the watched tag. Confirm Registry emits
   `tag_updated`, Cloud responds 202 after database persistence, and one
   digest-pinned deployment completes.
4. Replay the same delivery and confirm it does not create a second deploy.
   Push a different tag/repository and confirm the Cloud service is untouched.
5. Rotate the PAT, verify pulls continue, then disconnect and confirm the
   existing workload remains online while future Registry deploys stop.

## Explicitly deferred

- Registry OAuth/authorization-code flow so users do not paste a PAT.
- Per-user or per-repository webhook subscriptions and repository-filtered
  Registry event delivery. The first release relies on one administrator
  configured platform webhook.
- Supporting arbitrary registry hosts or using Cloud as an image publisher.
- Automatically stopping a service when a connection is revoked.
