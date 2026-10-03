# Knotree Cloud Frontend — UI/UX System Design

> **Status:** Implemented MVP, living design contract  
> **Scope:** `apps/web`  
> **Last updated:** 2026-10-04
> **Frontend:** Vite + React 19 + React Router + shadcn/Base UI + Tailwind CSS v4  
> **Production:** `https://cloud.knotree.com`  
> **API:** `https://cloudapi.knotree.com/api/v1`

This document is the source of truth for the frontend experience in the first
Knotree Cloud slice: email/password authentication, first-workspace onboarding,
project creation, and the initial topology dashboard. It documents both the
behavior already implemented and the rules future screens must preserve.

## 1. Product intent and scope

Knotree Cloud helps a person turn ideas into an organized workspace. The first
session should feel quick, calm, and trustworthy:

1. Create an account or sign in.
2. If the account has no workspace, create exactly one first workspace.
3. Continue into that workspace.

### In scope

- Login with email and password.
- Registration with full name, email, and password.
- Automatic authenticated session after registration or login.
- No email verification gate in development mode.
- First-workspace creation with a name; workspace URLs use the generated UUID.
- Success confirmation before entering the workspace.
- Existing-workspace destination after a later login.
- Session loading, field validation, API errors, sign out, and protected routes.
- Project creation with a suggested name-derived URL and optional custom slug.
- Workspace project index with empty, loading, error, and populated states.
- Project dashboard with a searchable resource list, separate Resources,
  Topology, Metrics, Logs, and Integrations views, and a resource workspace for
  editing or inspecting the selected service or data store.
- Docker App service deployment from a pasted image reference. Public images
  deploy without login; private `ghcr.io` images require a GitHub connection.
- Up to six Docker App services per project. New services start without a
  database; the selected service's Settings menu assigns or removes a
  project PostgreSQL connection over the private network.
- Project provisioning supports one PostgreSQL resource and one Redis
  resource. Hosted HTML pages use the App service workflow and include an HTML
  editor and analytics view.
- Resource workspace sections for Deployments, Database, Backups, Variables,
  Metrics, Console, and Settings. The Database section reads and mutates the
  selected project's dedicated PostgreSQL instance through typed API calls;
  Metrics reads live per-project runtime data through the same boundary.
- One app-wide light/dark theme, persisted as a local view preference and
  applied before first paint.
- Cloudflare Workers static-asset deployment with SPA fallback.

### Not in the current slice

- Password reset or email delivery.
- Other OAuth or social login providers.
- Multiple workspaces per user.
- Workspace switching, invitations, billing data, or settings mutations.
- Database backups and shell access to App service containers.

Each project supports one real PostgreSQL resource, one Redis resource, and up
to six Docker App services. Database tables, rows, schema creation, SQL
results, live stats, runtime metrics, and safe configuration settings come from
the dedicated PostgreSQL instance. App service and hosted HTML cards are backed
by the API and show their source, status, and returned service URL. Unsupported
operational sections remain explicitly unavailable until their APIs and product
contracts exist.

When a user assigns a database from an App service Settings menu, the
deployment card shows the project-private Postgres link. The Variables section
lists `DATABASE_URL` and the `PG*` assignments with password values masked.
Creating either resource leaves the other unchanged until the user chooses the
connection; the UI never asks the user to paste or copy a database password
into the app configuration.

## 2. Experience principles

### Calm and legible

Use generous whitespace, a white canvas, restrained borders, and a single
strong primary action. Copy should explain the next step without marketing
noise.

### Progressive commitment

Ask only for the information required at the current step. Account creation
and workspace creation are separate moments so the user understands what has
been created.

### One clear next action

Each screen has one dominant CTA:

- `Sign in` or `Create account` on authentication screens.
- `Create workspace` during onboarding.
- `Continue to workspace` after successful creation.
- `Create project` or `New project` at the workspace boundary.

### Feedback close to the cause

Validation is shown beneath the affected field. Transport or business errors
are shown in a page-level alert. Submit buttons become disabled and show a
spinner while a request is in flight.

### Trust by default

Do not expose session tokens or passwords in UI state that is persisted to
storage. Keep credential errors generic, make sign out visible, and preserve
keyboard and screen-reader affordances.

## 3. Information architecture

### Route map

| Route                                            | Access        | Screen                                        | Redirect rule                                                                                                               |
| ------------------------------------------------ | ------------- | --------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `/`                                              | Any           | Session destination                           | Anonymous → `/login`; authenticated without workspace → `/new/workspace`; authenticated with workspace → `/workspace/:workspaceId` |
| `/login`                                         | Public only   | Sign-in form                                  | Authenticated users are sent to their workspace destination                                                                 |
| `/register`                                      | Public only   | Registration form                             | Authenticated users are sent to their workspace destination                                                                 |
| `/new/workspace`                                 | Authenticated | First-workspace form or creation confirmation | An account that already has a workspace is sent to `/workspace/:workspaceId`                                                       |
| `/workspace/:workspaceId`                               | Authenticated | Workspace project index                       | No workspace → `/new/workspace`; a non-matching UUID → the account's own workspace                                         |
| `/workspace/:workspaceId/project/:projectSlug` | Authenticated | Project dashboard                             | Workspace mismatch → the account's own workspace; missing project → unavailable state                                       |
| Any other route                                  | Any           | None                                          | Redirect to `/`                                                                                                             |

The route guards live in `apps/web/src/App.tsx`. The workspace page also
protects against revisiting the creation route after the account already has a
workspace.

### Navigation model

Navigation is intentionally small at the workspace boundary:

- Brand mark identifies the product on auth/onboarding screens.
- Auth screens link only to the alternate auth mode.
- Every signed-in screen uses the shared `AppShell`: a sidebar on the canvas
  (workspace switcher, scope navigation, Integrations pinned to the bottom)
  and an inset panel with a breadcrumb header. The header always carries the
  page action, the theme toggle, and the account menu (identity, Integrations,
  `Sign out`).
- Workspace scope navigation: Projects. Project scope navigation: Resources,
  Topology, Metrics, Logs; the sidebar also shows the open project.
- The workspace success state provides the first forward transition into the
  workspace.

Auth remains a quiet entry shell; product navigation belongs at the workspace
and project boundaries.

## 4. Primary user journeys

### New user

```text
Open / or /register
  → registration form
  → client validation
  → POST /auth/register
  → authenticated session cookie
  → /new/workspace
  → workspace name
  → POST /workspaces
  → "Workspace created" confirmation
  → Continue to workspace
  → /workspace/:workspaceId
```

Registration signs the user in immediately. Because the account has no
workspace, the returned `workspace` value is `null` and the router selects
`/new/workspace`.

### Returning user with a workspace

```text
Open / or /login
  → POST /auth/login
  → session response includes workspace
  → /workspace/:workspaceId
```

### Returning user without a workspace

```text
Open / or /login
  → POST /auth/login
  → session response includes workspace: null
  → /new/workspace
```

### Existing authenticated session on page load

```text
App boot
  → request CSRF token and /auth/me in parallel
  → loading screen while both resolve
  → 401 from /auth/me means anonymous
  → any valid session is routed by workspace presence
```

### Sign out

```text
Click Sign out
  → POST /auth/logout with CSRF header
  → clear local session and CSRF cache regardless of response
  → replace history with /login
```

### Create a project and open its dashboard

```text
/workspace/:workspaceId
  → GET /workspaces/:workspaceId/projects
  → empty project state or project list
  → New project
  → project name + suggested URL; optionally customize the slug
  → POST /workspaces/:workspaceId/projects
  → navigate to /workspace/:workspaceId/project/:projectSlug
  → GET /workspaces/:workspaceId/projects/:projectSlug
  → topology dashboard
  → Add → Postgres
  → database display name
  → POST /workspaces/:workspaceId/projects/:projectSlug/resources
  → queued dedicated PostgreSQL instance + own volume + login role
  → poll the resource until connectivity is confirmed
  → ready Postgres card + resource workspace (or a safe capacity error)
  → Add → App service
  → service name + Docker image + public/private image source + container port
  → GitHub connection when the source is private
  → POST /workspaces/:workspaceId/projects/:projectSlug/app-services
  → Docker pull + isolated container + project-private network
  → no database variables until the user assigns a database in Settings
  → ready App service card + returned service URL
```

Workspace identity and authorization use the workspace UUID. Project creation
is scoped to that workspace. The API derives a normalized slug from the project
name when no custom slug is sent and enforces per-workspace uniqueness; the
client shows a suggested URL and validates a custom slug before submission.
PostgreSQL creation is scoped to the project and is idempotent: a retry resumes
a `provisioning`/`error` resource, while a ready resource is returned without
creating a second database.

## 5. Screen specifications

### 5.1 Authentication shell

Component: `AuthShell`
Used by: `/login`, `/register`

- Two columns from `lg`: a form column (brand, alternate-mode link, centered
  form no wider than `22.5rem`, quiet footer) and an inset showcase panel on a
  dotted canvas.
- The showcase is a static schematic of one service linked to its data stores
  over the private network, with the caption “Services, databases and the
  network between them, in one place.” It is `aria-hidden` and never presents
  itself as live data.
- Below `lg` the showcase is hidden; below `480px` the alternate-mode prompt
  keeps only its link.

### 5.2 Login

Heading: `Welcome back`  
Supporting text: `Sign in with your Knotree account to open your workspace.`

Fields:

| Order | Label    | Input behavior                       | Browser autocomplete |
| ----- | -------- | ------------------------------------ | -------------------- |
| 1     | Email    | Email input, trimmed before submit   | `email`              |
| 2     | Password | Password input with show/hide toggle | `current-password`   |

CTA: `Sign in`  
Secondary route: `Don't have an account? Create one`

The login form does not display password-reset or OAuth affordances because
those flows are not implemented.

### 5.3 Registration

Heading: `Create your account`  
Supporting text: `Start with a workspace for your ideas.`

Fields:

| Order | Label      | Input behavior                                     | Browser autocomplete |
| ----- | ---------- | -------------------------------------------------- | -------------------- |
| 1     | Full name  | Trimmed before submit                              | `name`               |
| 2     | Work email | Lowercase normalization occurs at the API boundary | `email`              |
| 3     | Password   | Hidden by default with show/hide toggle            | `new-password`       |

CTA: `Create account`  
Secondary route: `Already have an account? Sign in`

On success, the account is already authenticated. In development,
`emailVerified` may be `false`, but the user is allowed to continue because
`AUTH_REQUIRE_EMAIL_VERIFICATION=false`.

### 5.4 Create first workspace

Component: `NewWorkspacePage`
Route: `/new/workspace`

- Canvas background with the compact brand mark top-left.
- A single bordered card no wider than `26rem` holding a `Step 1 of 2`
  indicator, heading `Create your first workspace`, supporting text `A
  workspace is where your ideas come together.`, the workspace name field
  (required, 1–80 characters), and the `Create workspace` CTA.
- Helper text: `Projects and services can be added any time.`

The API generates a UUID for workspace identity; users do not choose a
workspace URL slug.

### 5.5 Creation confirmation

The same card shows a check mark on the brand-soft surface, `Workspace
created`, `<workspace name> is ready for your ideas.`, and a full-width
`Continue to workspace` CTA.

### 5.6 Workspace project index

Route: `/workspace/:workspaceId`

- Rendered inside `AppShell` with the breadcrumb `<workspace> / Projects` and
  `New project` as the header action.
- Page header: eyebrow `Workspace`, heading `Welcome to <workspace>`.
- Populated state: a `Projects` heading with the count, a project search, and a
  card grid. Each card shows the project initials tile, name, and mono `/slug`;
  a dashed `New project` card closes the grid.
- Empty state uses the heading `Create your first project`, a `Create project`
  CTA, and a three-step explainer (create, add services and data, deploy and
  observe).
- Loading uses an inline spinner row; list failures use a page-level alert
  while leaving the create action available.

The workspace route must only render the workspace returned for the current
session. A mismatching UUID is corrected by the route guard.

### 5.7 Project dashboard

Component: `ProjectHomePage` and `TopologyDashboard`
Route: `/workspace/:workspaceId/project/:projectSlug`

The dashboard opens to `Resources` so a project with several services and data
stores remains manageable. The `Topology` view keeps the interactive
infrastructure canvas. A dark theme uses the same layout and replaces the
page-local surface tokens.

#### Desktop composition

- `AppShell` chrome: sidebar with the workspace switcher, the open project
  (initials, name, `/slug`), Resources / Topology / Metrics / Logs, and
  Integrations at the bottom. The header breadcrumb is `<workspace> /
  <project>` plus an environment badge; `Add resource` is the primary action.
- `Add resource` opens a menu of Postgres, Redis, Images, and App service /
  HTML page, each with a one-line description. App service creation is capped
  at six services and the option says so when the cap is reached.
- The `Resources` view opens by default: a health strip (Resources, Healthy,
  Deploying, Needs attention — counted from real statuses), a segmented filter
  (`All resources`, `App services`, `Data stores`), search, and a table of
  name + mono detail, type, status badge, and `Open`. Rows are clickable. The
  empty state offers quick-create tiles for each resource type.
- The `Topology` view is a full-bleed dotted canvas with a legend, draggable
  resource cards (icon, name, type, mono detail, status, volume), dashed
  private-network connectors from the database to assigned services, and a
  zoom dock (zoom out, readout, zoom in, fit). Data stores carry a brand-colour
  edge.
- The `Metrics` view lists resources with runtime metrics as cards; each opens
  the resource Metrics tab, which renders real API samples with explicit
  loading, error, or unsupported-provider states.
- The `Logs` view shows deployment logs in a toolbar + stream panel and a
  truthful empty state until runtime log ingestion exists.
- Selecting a resource opens `ResourceWorkspace` as a right-hand drawer (at
  most `66rem` wide) with `Deployments`, `Database`, `Backups`, `Variables`,
  `Metrics`, `Console`, and `Settings` tabs (HTML pages add `Analytics` and
  drop `Database`/`Backups`). The drawer header shows the type, environment,
  and status.
- Database, Variables, Metrics, Settings, and Console behaviour is unchanged:
  live tables and bounded SQL, masked variables, ranged metrics with hover
  values, assign/remove a Postgres connection, and no shell access.

#### Context and transient state

- Workspace and environment controls are keyboard-operable menus.
- Environment options are `production` and `staging`; selection is local to
  the current page.
- Zoom is clamped from 80% to 125%; fit returns to 100%. Zoom and selected
  resource are persisted per project in local storage as view preferences.
- One global theme toggles between `light` and `dark` from the header, persists
  in local storage, and is applied by an inline script before first paint. The
  first visit follows `prefers-color-scheme`.
- The Logs rail item opens a project-scoped logs shell with resource filtering,
  search, and Live/Paused controls. It remains empty until log ingestion exists.
- Metrics samples are scoped to the selected dedicated PostgreSQL resource. In
  Docker development, CPU/memory/network/block-I/O values come from that
  container and volume usage comes from its mounted data path. A background
  sampler persists samples in the control-plane database for up to 30 days;
  the browser requests a bounded bucketed series and can switch ranges without
  losing the current project scope. Providers without a runtime metrics
  adapter expose an explicit unavailable message instead of fabricated values.
- Resource loading and provisioning failures keep the Add action available and
  show a safe, page-level message; raw database or Docker errors never reach
  the browser. PostgreSQL creation returns a durable provisioning resource and
  the dashboard polls it until ready or failed, so a slow Kubernetes scheduler
  cannot leave the create dialog waiting on an HTTP request.
- Activity, notifications, Agent, Resources, Settings, undo/redo, and
  layers provide explicit placeholder feedback until their APIs exist.
- Direct project lookup failures show `Project unavailable` and a `Back to
workspace` action.

#### Mobile composition

- Below `768px` the sidebar navigation docks to a fixed bottom bar; the header
  keeps a compact brand link, the current breadcrumb, the action (icon-only
  `Add resource`), theme toggle, and account menu.
- The health strip becomes two columns, the table hides the Type column, and
  the topology cards stack in a single column without connectors.
- `ResourceWorkspace` fills the viewport with a horizontally scrollable tab
  bar.
- The layout must remain within the viewport width at 320px and 390px.

The project card is a logical project/App service node. PostgreSQL and App
service topology data come only from their typed resource APIs; an absent App
service is shown as `Needs setup`, not as a fabricated online deployment.

### 5.8 Loading and unavailable states

`LoadingScreen` is rendered while the auth provider is bootstrapping. It is a
minimal centered loading surface and must not expose a partially evaluated
route.

During form submission:

- Keep the entered values in place.
- Disable the primary button.
- Show an inline spinner before the CTA label.
- Keep the layout height stable where possible.

When the API is unavailable, show the page-level message:
`The API is currently unavailable. Please try again.`

The auth provider also retains a `bootstrapError` for startup failures. Any
future global error UI should surface it without replacing the current route
until the user understands how to recover.

## 6. Design system

### 6.1 Visual language

“Instrument panel”: warm near-neutral greys, hairline borders, small radii,
and almost no shadow outside overlays. Ink (the foreground colour) is the
action colour; pine is the single brand accent for focus, selection, links,
and live state. Status colours (ok, warn, danger) are reserved for status and
always paired with text. Technical values — images, hosts, slugs, database
names, timestamps — are set in Geist Mono. Avoid gradients, glows, decorative
illustration, and oversized display type.

### 6.2 Color tokens

Tokens live in `apps/web/src/index.css` as OKLCH custom properties on `:root`
and are redefined under `.dark`. Components use semantic roles, never raw
colours.

| Token              | Role                                                    |
| ------------------ | ------------------------------------------------------- |
| `canvas`           | App background behind the shell and topology canvas     |
| `background`       | Panels, cards, inputs                                   |
| `surface`          | Table headers, quiet fills, hover                       |
| `surface-2`        | Active nav, segmented control track, pressed states     |
| `border` / `border-strong` | Hairlines / hover and input borders             |
| `foreground`       | Primary text and primary button fill                    |
| `muted-foreground` | Secondary text                                          |
| `faint`            | Tertiary text, placeholders, idle dots                  |
| `brand`            | Focus ring, selection, active nav icon, links           |
| `ok` / `warn` / `danger` / `info` | Status only                              |
| `chart-*`          | Metric series (blue, violet, green, orange; ink = fg)   |
| `code-bg`          | Log and SQL surfaces                                    |

The shadcn roles (`primary`, `secondary`, `muted`, `accent`, `destructive`,
`input`, `ring`, …) are mapped onto these tokens, so primitives follow the
theme automatically. The `--project-*` aliases in `project-home.css` exist
only so older component styles resolve to the same tokens.

### 6.3 Typography

- Geist Variable for UI text, Geist Mono Variable for technical values
  (`@fontsource-variable/geist`, `@fontsource-variable/geist-mono`).
- Base size 14px. Page titles 22px semibold with tight tracking; section
  titles 15px; table and control text 13px; meta text 12px.
- Numbers in stats and charts use tabular figures.

### 6.4 Spacing, shape, and elevation

- Base radius `0.5rem`; cards and tables `0.625rem`; the shell panel and
  overlays `0.75rem`; controls `0.375rem`.
- Controls are 32px high (`h-8`); primary auth/onboarding CTAs are 36–40px.
- Elevation: `shadow-sm` for the shell panel and active nav, `shadow-md` for
  hover and canvas nodes, `shadow-lg` for menus, dialogs, drawers, and toasts.

### 6.5 Iconography and imagery

- Use `lucide-react` icons at 14–16px with a 1.75 stroke for controls,
  resource types, and navigation.
- The Knotree glyph (`KnotreeGlyph`) is an inline SVG of three linked nodes;
  `BrandMark` pairs it with the product name.
- Current icons: eye/eye-off for password visibility, check for success, and
  log-out for sign out.
- Decorative images use an empty `alt` attribute. Product-critical information
  must remain in text.

### 6.6 Topology canvas

The canvas uses the `canvas` token with a 22px dot grid. Nodes are 248px wide,
positioned by percentage (centre x, top edge) inside an inset world that zooms
from 80% to 125%. Connectors are dashed `border-strong` paths that turn solid
`brand` when an endpoint is selected; selection adds a brand ring to the node.

## 7. Component architecture

### Application composition

```text
StrictMode
  └─ BrowserRouter
      └─ AuthProvider
          └─ App / route guards
              ├─ AuthShell
              │   ├─ BrandMark
              │   └─ AuthPage
              │       └─ PasswordField
              ├─ NewWorkspacePage
              │   └─ WorkspaceCreated
              └─ WorkspacePage
                  ├─ ProjectCreateDialog
                  └─ ProjectHomePage
                      ├─ PostgresCreateDialog
                      ├─ TopologyDashboard
                      ├─ ResourceWorkspace
                      └─ LogsWorkspace
```

### Component inventory

| Component                                                    | Responsibility                                                                   | Reuse rule                                                                     |
| ------------------------------------------------------------ | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| `AuthProvider`                                               | Bootstrap session, expose auth/workspace commands                                | Keep auth mutations here; pages should not own session synchronization         |
| `AuthShell`                                                  | Shared auth composition and showcase panel                                       | Use for public auth entry screens only                                         |
| `AppShell`, `SidebarNavItem`                                 | Signed-in chrome: sidebar, breadcrumb header, theme toggle, account menu         | Every signed-in screen; pass scope navigation rather than re-implementing it   |
| `ThemeProvider`                                              | App-wide light/dark theme and persistence                                        | Read it with `useTheme`; never store a second, page-local theme                |
| `BrandMark`                                                  | Product identity, compact or full size                                           | Keep `alt=""` because adjacent text carries the name                           |
| `PasswordField`                                              | Password input plus visibility toggle                                            | Always provide correct `autocomplete` and field error                          |
| `LoadingScreen`                                              | Auth bootstrap loading state                                                     | Use before protected/public route decisions are known                          |
| `AuthPage`                                                   | Login/register form and local validation                                         | Mode is explicit: `login` or `register`                                        |
| `NewWorkspacePage`                                           | First workspace form and success state                                           | Must not become a general workspace CRUD screen without a new contract         |
| `WorkspacePage`                                              | Project index, empty state, project list, and sign out                           | Keep workspace-level project selection here                                    |
| `ProjectCreateDialog`                                        | Create a project with a suggested URL and optional custom slug                  | Keep the name-derived URL as the default; API remains authoritative             |
| `PostgresCreateDialog`                                       | Request one real PostgreSQL resource and show pending/error states               | Never collect or persist database passwords in the browser                     |
| `ProjectHomePage`                                            | Load one project and render unavailable/loading states                           | Keep route data fetching typed and scoped to the current workspace             |
| `TopologyDashboard`                                          | Topbar, view navigation, resource list, topology, metrics, logs, menus, and themes | Keep unsupported resource actions explicit until their provisioning APIs exist |
| `ResourceWorkspace`                                          | Accessible resource sheet, live database management views, and safe copy        | Keep operational data truthful; never fabricate tables, metrics, or logs       |
| `LogsWorkspace`                                              | Project-scoped log toolbar and unavailable/empty state                           | Add streaming/query APIs before rendering runtime events                       |
| `Button`, `Field`, `Input`, `InputGroup`, `Alert`, `Spinner` | shadcn/Base UI primitives                                                        | Prefer composition and variants over bespoke controls                          |

UI primitives are generated/configured through shadcn and backed by Base UI.
Keep behavior accessible at the primitive layer; page code should supply
labels, values, and domain states.

### File boundaries

```text
apps/web/src/
├─ auth/             session context and auth commands
├─ components/       product-level composition
│  ├─ resource-workspace.tsx
│  ├─ resource-workspace.css
│  └─ ui/             shadcn/Base UI primitives
├─ lib/               API client, types, slug normalization, utilities
├─ pages/             route-level screens and topology styles
│  ├─ project-home-page.tsx
│  └─ project-home.css
├─ App.tsx            route table and guards
├─ main.tsx           runtime providers
└─ index.css         tokens and global layout rules
```

Keep server/API concerns in `lib/api.ts` and typed models. Do not call
`fetch` directly from a page when the request changes session or CSRF state.
Project requests are grouped in `lib/projects.ts`; pages consume those typed
functions rather than constructing project URLs inline.
PostgreSQL resource requests are grouped in `lib/resources.ts`; connection
details are rendered from the authenticated API response and are not written to
local storage. Database management requests use the same module and always
include the workspace, project, and resource scope.

## 8. Frontend state and data flow

### Auth state machine

```text
loading
  ├─ /auth/me = 200 → ready + session
  ├─ /auth/me = 401 → ready + anonymous
  └─ other bootstrap failure → ready + bootstrapError

ready + anonymous
  ├─ signIn/signUp success → ready + session
  └─ auth error → remain anonymous; show form error

ready + session without workspace
  └─ createWorkspace success → session.workspace populated

ready + session
  └─ signOut → anonymous + CSRF cache reset
```

### Project dashboard state machine

```text
workspace route
  ├─ listProjects pending → loading state
  ├─ listProjects success + [] → empty project state
  ├─ listProjects success + projects → project list
  └─ listProjects failure → alert + create action remains available

project route
  ├─ getProject pending → loading state
  ├─ getProject success → topology dashboard
  └─ getProject failure → project unavailable + back action

topology dashboard
  ├─ listPostgresResources pending → resource loading state
  ├─ listPostgresResources success + [] → empty database state
  ├─ listPostgresResources success + resource → real Postgres node
  ├─ listPostgresResources failure → resource error + Add remains available
  ├─ createPostgresResource pending → disabled dialog + spinner
  ├─ createPostgresResource success → ready node + resource workspace
  ├─ createPostgresResource failure → safe dialog/page error
  ├─ select node → selected card + resource workspace
  ├─ Database tab → list live tables or truthful empty/error state
  ├─ select table → column metadata + bounded live rows
  ├─ create table/query/stats/config action → API result + local feedback
  ├─ copy connection action → clipboard + polite toast
  └─ zoom/menu/theme/log action → local view state + toast where useful
```

### State ownership

| State                                                                 | Owner                                   | Persistence                                                                                         |
| --------------------------------------------------------------------- | --------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `status` (`loading`/`ready`)                                          | `AuthProvider`                          | Memory only                                                                                         |
| `session`                                                             | `AuthProvider`                          | Memory; server cookie is authoritative                                                              |
| `bootstrapError`                                                      | `AuthProvider`                          | Memory only                                                                                         |
| CSRF token cache                                                      | `lib/api.ts`                            | Memory only; CSRF cookie is server-managed                                                          |
| Form values/errors/loading                                            | Route page                              | Reset on page mount/navigation                                                                      |
| `createdWorkspace` confirmation                                       | `NewWorkspacePage`                      | Memory only; session is updated by provider                                                         |
| `projects` and project-list error                                     | `WorkspacePage`                         | Memory only; refetched on workspace page mount                                                      |
| Project create form                                                   | `ProjectCreateDialog`                   | Reset when dialog closes/reopens                                                                    |
| `project`, load error, selected node, menus, toast                    | `ProjectHomePage` / `TopologyDashboard` | Project data is API-backed; view preferences persist locally                                        |
| `postgresResource`, resource loading/error, create dialog, copy state | `TopologyDashboard`                     | Database metadata and credentials are API-backed; connection string is held in component state only |
| Zoom and selected resource                                            | `TopologyDashboard`                     | `localStorage` keyed by project id                                                                  |
| Global theme                                                          | `ThemeProvider`                         | `localStorage` key `knotree-theme` (the legacy `project-topology-dashboard-theme` is read once)      |
| Resource tab, table search, variables search, logs filters            | `ResourceWorkspace` / `LogsWorkspace`   | Memory only; reset when the surface unmounts                                                        |

The frontend does not store the session token, password, or workspace
membership in local storage.

### API request behavior

`apiRequest`:

1. Adds `Accept: application/json`.
2. Serializes request bodies as JSON.
3. Includes credentials for cross-origin cookies.
4. Obtains and caches a CSRF token for every non-GET request.
5. Converts JSON error envelopes into typed `ApiError` instances.

The session cookie is HTTP-only and is never read by JavaScript. A page uses
the typed response from the API rather than trying to infer authentication from
browser storage.

## 9. API contract consumed by the frontend

### Base URL

| Environment       | Value                                 |
| ----------------- | ------------------------------------- |
| Local development | `http://localhost:8080/api/v1`        |
| Production        | `https://cloudapi.knotree.com/api/v1` |

`VITE_API_BASE_URL` overrides the default. The production fallback is explicit
so a production build cannot silently point at localhost.

### Endpoints

| Method | Path                                                         | Auth                        | Frontend use                                       |
| ------ | ------------------------------------------------------------ | --------------------------- | -------------------------------------------------- |
| `GET`  | `/auth/csrf`                                                 | No                          | Seed CSRF cookie and return `{ csrfToken }`        |
| `POST` | `/auth/register`                                             | CSRF                        | Create user and start session                      |
| `POST` | `/auth/login`                                                | CSRF                        | Authenticate and return current workspace if any   |
| `GET`  | `/auth/me`                                                   | Session cookie              | Restore session on boot                            |
| `POST` | `/auth/logout`                                               | Session + CSRF              | Revoke session and clear cookies                   |
| `POST` | `/workspaces`                                                | Session + CSRF              | Create the account's first workspace               |
| `GET`  | `/workspaces/:workspaceId`                                          | Session + membership        | Load workspace identity by UUID                   |
| `GET`  | `/workspaces/:workspaceId/projects`                        | Session + membership        | List projects in the current workspace             |
| `POST` | `/workspaces/:workspaceId/projects`                        | Session + membership + CSRF | Create a project                                   |
| `GET`  | `/workspaces/:workspaceId/projects/:projectSlug`           | Session + membership        | Load one project for the dashboard                 |
| `GET`  | `/workspaces/:workspaceId/projects/:projectSlug/resources` | Session + membership        | Load persisted PostgreSQL resources                |
| `POST` | `/workspaces/:workspaceId/projects/:projectSlug/resources` | Session + membership + CSRF | Provision or retry the project PostgreSQL resource |
| `GET`  | `.../resources/:resourceId/database/tables`                 | Session + membership        | List live project tables                           |
| `POST` | `.../resources/:resourceId/database/tables`                 | Session + membership + CSRF | Create a validated project table                  |
| `GET`  | `.../resources/:resourceId/database/table-data`             | Session + membership        | Read paginated rows and column metadata            |
| `GET`  | `.../resources/:resourceId/database/stats`                  | Session + membership        | Load live database statistics                      |
| `GET`  | `.../resources/:resourceId/database/metrics?range=1h\|6h\|24h\|7d\|30d` | Session + membership        | Load retained CPU, memory, volume, network, and disk metrics |
| `GET`  | `.../resources/:resourceId/database/config`                 | Session + membership        | Load allowlisted PostgreSQL settings              |
| `POST` | `.../resources/:resourceId/database/query`                  | Session + membership + CSRF | Run one bounded SQL statement                     |

The metrics endpoint records ready dedicated resources every five seconds in
the control-plane database and retains at most 30 days per resource. The
`range` query selects one of `1h`, `6h`, `24h`, `7d`, or `30d`; the server
chooses a bucket resolution and returns at most 300 chronological real sample
points. Runtime byte counters are cumulative totals from the selected dedicated
container; `volumeUsedBytes` and `volumeCapacityBytes` are filesystem byte
values. The UI uses the returned timestamp/value pairs to show a nearest-point
tooltip on hover. A provider that cannot expose runtime metrics returns `null`
fields plus `systemMetricsAvailable: false` and a user-safe
`systemMetricsMessage`.

```json
{
  "provider": "docker",
  "systemMetricsAvailable": true,
  "sampleIntervalSeconds": 5,
  "retentionSeconds": 2592000,
  "range": "24h",
  "fromTimestamp": 1778611200,
  "toTimestamp": 1778697600,
  "resolutionSeconds": 288,
  "points": [{
    "timestamp": 1778697600,
    "cpuPercent": 2.5,
    "memoryUsedBytes": 11010048,
    "memoryLimitBytes": 1073741824,
    "volumeUsedBytes": 33554432,
    "volumeCapacityBytes": 10737418240,
    "networkReceiveBytes": 1500,
    "networkTransmitBytes": 2097152,
    "diskReadBytes": 3000000,
    "diskWriteBytes": 4294967296
  }]
}
```

### Shared success shape

```json
{
  "user": {
    "id": "uuid",
    "fullName": "Jane Doe",
    "email": "jane@example.com",
    "emailVerified": false
  },
  "workspace": {
    "id": "uuid",
    "name": "Acme Studio"
  }
}
```

`workspace` is `null` until the first workspace is created.

Project list and detail endpoints return the compact project shape:

```json
{
  "id": "uuid",
  "name": "Knotree Study",
  "slug": "knotree-study"
}
```

`POST /workspaces/:workspaceId/projects` accepts `{ "name": "..." }` and may
include an optional custom `slug`. When omitted, the server derives a slug
from the project name. Slugs are normalized and unique within the workspace. A
duplicate returns `PROJECT_SLUG_TAKEN`; an inaccessible workspace or project
returns the corresponding not-found envelope.

The resource list returns zero or one PostgreSQL resource for the project:

```json
{
  "id": "uuid",
  "name": "Postgres",
  "resourceType": "postgres",
  "status": "ready",
  "databaseName": "knotree_db_<project-id>",
  "username": "knotree_role_<project-id>",
  "host": "localhost",
  "port": 5432,
  "clusterProvider": "docker",
  "clusterName": "knotree-pg-<project-id>",
  "connectionString": "postgres://..."
}
```

`POST /workspaces/:workspaceId/projects/:projectSlug/resources` accepts
`{ "resourceType": "postgres", "name": "Postgres" }`. The database and role
are created inside a dedicated Docker or Kubernetes provider instance selected
by the API configuration. A repeated request is idempotent for the project and
returns the existing ready resource.

The database sub-resources are scoped to the resource UUID. `tables` returns
table metadata, `table-data` returns columns and bounded rows, `POST tables`
accepts allowlisted PostgreSQL types, `stats` and `config` read live values,
and `query` returns rows or affected-row counts. The UI maps loading and safe
API errors to local states and never invents database data.

### Error envelope

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Please check the highlighted fields.",
    "fields": {
      "email": "Enter a valid email address."
    }
  }
}
```

The UI maps `fields` by field name and uses `message` for the page-level alert.
Important current codes include `EMAIL_IN_USE`, `INVALID_CREDENTIALS`,
`WORKSPACE_EXISTS`, `AUTHENTICATION_REQUIRED`, and
`EMAIL_NOT_VERIFIED`, `PROJECT_SLUG_TAKEN`, `PROJECT_NOT_FOUND`, and
`WORKSPACE_NOT_FOUND`, `DATABASE_PROVISIONING_DISABLED`, and
`DATABASE_PROVISIONING_FAILED`.

## 10. Validation and interaction matrix

| Screen       | Condition                                   | UI response                                                                   |
| ------------ | ------------------------------------------- | ----------------------------------------------------------------------------- |
| Login        | Invalid email                               | Field error: `Enter a valid email address.`                                   |
| Login        | Empty password                              | Field error: `Password is required.`                                          |
| Login        | Wrong credentials                           | Generic page alert; do not reveal whether email exists                        |
| Register     | Empty/too-long name                         | Field error for full name                                                     |
| Register     | Invalid email                               | Field error for email                                                         |
| Register     | Password outside 8–128 characters           | Field error: `Use 8 to 128 characters.`                                       |
| Register     | Existing email                              | API field error/page alert for `EMAIL_IN_USE`                                 |
| Workspace    | Empty/too-long name                         | Field error for name                                                          |
| Workspace    | Second creation attempt                     | API conflict `WORKSPACE_EXISTS`; route normally redirects existing users away |
| Project      | Empty/too-long name                         | Field error for project name                                                  |
| Project      | Invalid custom slug                         | Field error for lowercase project URL slug                                    |
| Project      | Suggested or custom slug already used       | API conflict `PROJECT_SLUG_TAKEN`; keep the custom URL action available       |
| Project home | Missing or inaccessible project             | `Project unavailable` state with a back-to-workspace action                   |
| Database     | Empty/too-long name                         | Field error for database name                                                 |
| Database     | Provisioning permissions or cluster failure | Safe API error; retain dialog action and never show raw SQL                   |
| Database     | Successful provisioning                     | Ready card, resource workspace, and copyable connection action                |
| Any submit   | Request pending                             | Disable CTA and show spinner                                                  |
| Any API call | Network/unknown failure                     | Page-level unavailable message                                                |

Validation is duplicated intentionally at the client and API boundaries:
client validation gives immediate feedback, while the Rust API remains the
source of truth for security, normalization, uniqueness, and authorization.

## 11. Responsive behavior

### Breakpoints

The current CSS uses Tailwind's standard responsive breakpoints:

- `<768px`: compact single-column composition and reduced page padding.
- `≥768px` (`md`): larger outer padding and form spacing.
- `≥1024px` (`lg`): auth two-column frame and full-height brand panel.

### Responsive rules

- Minimum supported viewport width is `320px`.
- No horizontal scrolling should be introduced by the URL prefix field.
- The primary CTA remains full width on auth/onboarding screens.
- Supporting email text in the workspace header hides below `sm`.
- Heading sizes step down on narrow screens while preserving hierarchy.
- Decorative artwork may crop; content and controls may not.
- Touch targets must remain usable at 100% and 200% zoom.
- Project topology uses a fixed 60px bottom rail below `768px` and reserves
  bottom padding so cards are not hidden behind it.
- Project topbar hides the project badge and Agent action on narrow screens;
  workspace context truncates instead of creating horizontal overflow.
- Topology cards become a single-column stack; connectors are hidden on mobile.
- The resource workspace fills the mobile viewport and locks page scrolling;
  its tab row scrolls horizontally inside the sheet without widening the
  document.

## 12. Accessibility contract

Target WCAG 2.2 AA for all new work in this surface.

- Use semantic landmarks: `main`, `aside`, `section`, and `header`.
- Every input has a visible `FieldLabel` connected through `htmlFor`/`id`.
- Invalid fields expose `aria-invalid`; field errors use `role="alert"`.
- Page-level API errors use the alert primitive and readable text, not color
  alone.
- Password visibility is a real button with an explicit accessible label and
  pressed state.
- Decorative brand artwork has empty alt text and does not interrupt the
  reading order.
- Focus-visible rings use the semantic `ring` token and must remain visible on
  white backgrounds.
- Keyboard order follows visual order: brand (non-interactive) → heading →
  fields → primary CTA → alternate route.
- Loading and disabled states must not trap focus or make the form impossible
  to recover.
- Do not put validation messages only in placeholders or tooltips.
- Respect reduced motion for any future animation; the current UI uses no
  essential animation.
- Topology resource cards are keyboard-focusable buttons with `aria-label` and
  `aria-pressed`; the selected resource uses a labelled modal dialog.
- The resource workspace uses `role="dialog"`, `aria-modal`, an explicit close
  label, Escape-to-close, a tablist with selected tabs, and a polite copy
  confirmation. The connection string is never rendered as plaintext.
- Icon-only topology actions have explicit accessible labels, and menu state is
  exposed through `aria-expanded`/`aria-controls`.
- Toast feedback uses a polite live region; the zoom readout is also announced
  as a live value.

## 13. Security and privacy UX rules

- All API requests use `credentials: "include"` for the cross-origin session.
- The frontend sends the CSRF header for non-GET requests; the server validates
  the exact allowed origin and double-submit token.
- Do not expose cookie values, passwords, or raw API response bodies in logs,
  analytics, error boundaries, or URLs.
- Use the server's generic invalid-credential message without client-side
  account enumeration.
- Treat a `401` from `/auth/me` as an anonymous session; other bootstrap
  failures are recoverable API availability problems.
- Development defaults to no email-verification requirement. Production
  defaults to verification required; a future email flow must add an explicit
  pending/verified UX before enabling it in production.
- Workspace access is always derived from the current authenticated session;
  never trust a workspace UUID or project slug in the route as proof of
  membership.
- Project list/create/detail requests are scoped through the authenticated
  workspace membership; route identifiers are never treated as authorization.

## 14. Runtime and deployment design

### Build pipeline

```text
Vite source
  → TypeScript build
  → Vite static bundle in apps/web/dist
  → Wrangler assets upload
  → Cloudflare Worker custom domain
```

`apps/web/wrangler.jsonc` configures:

- Worker name: `knotree-cloud-web`.
- Static asset directory: `./dist`.
- SPA fallback: `not_found_handling: "single-page-application"`.
- Custom domain route: `cloud.knotree.com`.
- Worker observability enabled.

The SPA fallback is required so direct visits to `/login`, `/new/workspace`,
`/workspace/:workspaceId`, and `/workspace/:workspaceId/project/:projectSlug` resolve
to the React application.

### Environment contract

| Variable            | Local default                  | Production expectation                |
| ------------------- | ------------------------------ | ------------------------------------- |
| `VITE_API_BASE_URL` | `http://localhost:8080/api/v1` | `https://cloudapi.knotree.com/api/v1` |

Only `VITE_*` values are embedded in the browser bundle. Secrets belong to the
API/deployment environment and must never be placed in the frontend env file.

## 15. Quality and verification

The frontend quality gates are:

```powershell
pnpm typecheck:web
pnpm lint:web
pnpm test:web
pnpm build:web
```

Current behavior coverage includes:

- Registration routes to `/new/workspace`.
- First workspace creation routes to `/workspace/:workspaceId` after confirmation.
- Existing workspace login routes directly to that workspace.
- Project URL suggestions normalize accents and kebab-case; the API remains
  authoritative when the suggested URL is submitted.
- Project creation routes to `/workspace/:workspaceId/project/:projectSlug`.
- Project list/detail API responses drive the workspace list and project dashboard.
- Dashboard interactions cover resource search/filter/open, Resources/Topology/
  Metrics/Logs/Integrations navigation, Add menu feedback, resource tabs,
  connection copy, theme controls, responsive navigation, and mobile layout.
- The Metrics view opens real API metrics for supported resources, exposes the
  live polling and range controls, and shows hover details for historical
  samples.

Every new route or meaningful interaction should add:

1. a route/interaction test;
2. an error and pending-state assertion;
3. a responsive/accessibility check;
4. a production build check when environment or routing changes.

### Manual visual QA checklist

- Check 320px, 390px, 768px, 1024px, and desktop widths.
- Confirm the project URL preview and custom slug input fit without overflow.
- Tab through every form without a mouse.
- Toggle password visibility without submitting the form.
- Verify errors are readable and do not shift the primary CTA unpredictably.
- Refresh `/new/workspace` and `/workspace/:workspaceId` while authenticated.
- Refresh a project deep link while authenticated and verify the project reloads.
- Check topology selection, resource workspace close, Add → Postgres, pending
  and error states, Deployments/Database tabs, table creation, live rows, SQL
  query results, stats/config loading, no-table empty state,
  connection-string copy, Metrics loading/error/live states and metric cards,
  theme toggles, Logs navigation, environment menu, zoom, and sign-out actions
  with keyboard and pointer input.
- Check the resource workspace at desktop and mobile widths, including tab
  scrolling and document-width preservation.
- Open direct deep links through the Cloudflare SPA fallback.
- Verify production bundles point to `cloudapi.knotree.com`, not localhost.

## 16. Extension rules for future frontend work

When adding a feature to this frontend:

1. Add the user journey and route/access rule to this document first.
2. Define loading, empty, success, validation, unauthorized, and unavailable
   states before implementing the happy path.
3. Reuse semantic tokens and existing Base UI/shadcn primitives.
4. Keep server state in a typed API boundary and auth state in `AuthProvider`.
5. Keep page-specific transient state local unless multiple routes need it.
6. Preserve the auth/onboarding shell as a quiet entry experience; product
   density belongs in workspace/project shells.
7. Update tests and this document in the same slice.

### Planned evolution points

- Extend the resource API and provisioning abstraction to Redis and app
  services, with explicit lifecycle and deletion contracts.
- Add persistent topology editing only with an explicit graph/resource model.
- Add a workspace selector only when multiple memberships are supported by the
  API and data model.
- Add password reset with explicit pending/success/failure states.
- Add email verification UX and a resend path before turning the production
  verification flag on for real users.
- Replace Logs, Console, Backups, and write-oriented Settings placeholders with
  API-backed data and explicit loading/error contracts. Add retention/rollup
  jobs for larger-than-30-day observability needs when the product contract
  expands.
- Add a dedicated query/cache layer if workspace data becomes larger than the
  current session response.

## 17. Source map

| Concern                          | Source                                                                                                     |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| Routes and guards                | `apps/web/src/App.tsx`                                                                                     |
| Auth/session state               | `apps/web/src/auth/auth-context.tsx`                                                                       |
| API and CSRF client              | `apps/web/src/lib/api.ts`                                                                                  |
| Shared types                     | `apps/web/src/lib/types.ts`, `apps/web/src/lib/auth-types.ts`                                              |
| Auth screens                     | `apps/web/src/pages/auth-page.tsx`                                                                         |
| Workspace screens                | `apps/web/src/pages/workspace-page.tsx`                                                                    |
| Project API boundary             | `apps/web/src/lib/projects.ts`                                                                             |
| PostgreSQL resource API boundary | `apps/web/src/lib/resources.ts`                                                                            |
| Project creation dialog          | `apps/web/src/components/project-create-dialog.tsx`                                                        |
| PostgreSQL creation dialog       | `apps/web/src/components/postgres-create-dialog.tsx`                                                       |
| Project dashboard                | `apps/web/src/pages/project-home-page.tsx`, `apps/web/src/pages/project-home.css`                          |
| Product composition              | `apps/web/src/components/`                                                                                 |
| Design tokens and layout CSS     | `apps/web/src/index.css`                                                                                   |
| Frontend deployment              | `apps/web/wrangler.jsonc`                                                                                  |
| Browser-facing API contract      | `apps/api/src/auth.rs`, `apps/api/src/workspaces.rs`, `apps/api/src/projects.rs`, `apps/api/src/models.rs` |
