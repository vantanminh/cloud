# Knotree Cloud MCP

The MCP server lets an authorized AI agent help a user debug and deploy App services.

## Endpoints

- `GET /.well-known/oauth-authorization-server`
- `GET /.well-known/oauth-protected-resource`
- `POST /oauth/register` — dynamic client registration
- `GET /oauth/authorize` — user approval (session cookie)
- `POST /oauth/token` — `authorization_code` and `refresh_token`
- `POST /mcp` — JSON-RPC tools with `Authorization: Bearer`

Access tokens expire; clients must use the refresh token to obtain a new access token. Tokens are bound to the authorizing user. Calls against another account fail.

## Tools

- `docs_search` / `docs_read` — product docs for deploy and debug
- `account_logs` — recent deployments for the user
- `service_logs` — runtime logs for one App service
- `list_resources` — Postgres, Redis, App services
- `create_redis` — provision Redis on the private network
- `deploy_app_service` — deploy from an image
- `setup_public_access` — enable or disable `*.knotree.org` and rate limits
