# Knotree Cloud PaaS guide

Knotree Cloud runs App services, PostgreSQL, and Redis for a project. This phase is not full Railway.com parity: there is no billing, teams, custom domains beyond `*.knotree.org`, or source builds from Git.

## Resource caps

Every tenant workload is hard-capped at:

- 1 vCPU
- 1 GiB RAM
- 10 GiB storage

Caps are applied when the container or Kubernetes object is created. A noisy App service cannot take the whole node.

## Public hostnames

An App service does **not** receive a public hostname until the owner enables public access. When enabled, Knotree assigns a random `*.knotree.org` hostname (for example `app-0123456789abcdef.knotree.org`). Disable public access and the hostname is no longer served.

## Rate limits

Public App services go through Kong with a per-service requests-per-minute limit (default 60). Change the limit in App service settings. Traffic over the limit is rejected for that service only.

## Redis

Add Redis from the topology board. The instance joins the project private network as hostname `redis`. Apps can use `REDIS_URL=redis://:password@redis:6379`.

## Deploy an App service

1. Push a container image (public registry or GHCR).
2. Add an App service and set the image and container port.
3. Optionally attach PostgreSQL from Settings.
4. Enable public access only when you want `*.knotree.org`.
5. Watch Deployments and runtime logs if the container fails.

Images are never built on the Knotree control-plane host. Build in CI, then deploy the image.

## MCP

AI agents can connect to the Knotree MCP server (`/mcp`) after OAuth. The agent can search these docs, read account and service logs, and create Redis / deploy App services for the authorized user.
