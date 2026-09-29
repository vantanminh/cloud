# GitHub-owned production configuration

Production deploy reads only GitHub Actions Secrets/Variables. No VPS `.env`, protected credential input file, copied pull secret, or server-generated key/password is used. Kubernetes Secrets are runtime outputs delivered by CI, not configuration inputs.

Set these under **Settings → Secrets and variables → Actions** (or organization values restricted to this repository):

| GitHub location | Name | Value |
| --- | --- | --- |
| Variable | `K8S_CONFIG_JSON` | Complete JSON from `deploy/ci/config.example.json`; all values are strings. Review domains and feature flags. |
| Secret | `K8S_SECRETS_JSON` | Complete JSON from `deploy/ci/secrets.example.json`, with every required empty placeholder filled. |
| Secret | `KUBE_CONFIG` | Production kubeconfig whose API server is `https://15.235.210.66:6443`. Same secret used by the other app workflows. |

`GHCR_USERNAME` and `GHCR_TOKEN` inside the secret JSON must be persistent credentials with package read access; the short-lived Actions `GITHUB_TOKEN` is used to publish images only. They are not copied from another server namespace.

`deploy/ci/contract.json` is the authoritative required/optional key list. Missing JSON, missing keys, blank required values, duplicate JSON keys, unknown keys, unsafe production settings and incomplete optional feature secret groups fail the **deploy-preflight** job before image publication or any Kubernetes write. PR checks use synthetic fixtures and never require production secrets. The production workflow runs automatically on the default branch; Accounts no longer silently skips deploy when `K3S_DEPLOY_ENABLED` is unset.

GitHub CI applies only the target namespace's ConfigMap/Secrets with kubectl and the `KUBE_CONFIG` secret, then removes the temporary kubeconfig on exit. Values are never passed as shell command arguments, sourced, printed, or uploaded as artifacts. Runtime checksums trigger only the affected application rollout when configuration changes. The target workload's existing PVC/data is preserved.

For an existing installation, seed GitHub with the exact current database password/URL and encryption keys. The deploy compares protected live credentials before any Kubernetes write and fails if they differ; updating a Secret is not a PostgreSQL password rotation or data re-encryption. Use a separate reviewed rotation flow for these changes. The deploy never silently reuses server values when a GitHub value is missing.

`POSTGRES_USER` and `POSTGRES_DB` are GitHub config values and must match `DATABASE_URL` and the live control-plane PostgreSQL StatefulSet. A mismatch stops deployment before it writes runtime Secrets or changes Helm resources. The remote deploy also requires k3s Secret encryption to be enabled with re-encryption complete; otherwise it fails before applying runtime values.

Local verification: `python3 -m unittest discover -s deploy/ci -p 'test_*.py'`. Real image builds and backend tests remain GitHub CI tasks. Local code/contract checks alone do not prove deployment.

## Shared Cloud/Registry webhook

Set a dedicated GitHub Secret `KNOTREE_REGISTRY_WEBHOOK_SECRET` with the **same random value of at least 32 characters** in both Cloud and Registry, preferably one organization secret exposed to these two repos. Do not put this key inside `K8S_SECRETS_JSON`; CI injects the dedicated Actions Secret into the runtime payload. Registry's `CLOUD_WEBHOOK_URL` comes from its config JSON. Registry configures the signed `tag_updated` endpoint with a stable ID after loading durable state, so re-deploying does not create duplicate managed hooks or generate a secret on the server. Cloud verifies the same signing secret before enqueuing deployments.

Production contracts require Accounts SSO enabled. Deploy and verify Accounts before enabling these production workflows. Optional GitHub OAuth credentials (Cloud), R2 storage and edge download credentials (Registry) are supplied in the secret JSON when those features are enabled; incomplete pairs/groups fail preflight.

## Cloud Helm and frontend

The API container loads the CI ConfigMap and Secret via `envFrom`; Helm's path/routing settings are generated from the same GitHub config JSON. The frontend's `VITE_API_BASE_URL` is validated and exported by preflight, then baked by the GitHub build job. The production chart no longer generates random credentials. Deployment scripts/chart are sent from the checked-out CI revision, without reading the VPS checkout or fetching GitHub on the server.

Cloud deploys both images by immutable CI digest. Re-running CI with a changed frontend variable therefore uses the newly built bundle, even when the source commit SHA is unchanged.
