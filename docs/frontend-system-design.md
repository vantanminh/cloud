# Knotree Cloud Frontend — UI/UX System Design

> **Status:** Implemented MVP, living design contract  
> **Scope:** `apps/web`  
> **Last updated:** 2026-09-13
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
- First-workspace creation with generated, editable URL slug.
- Success confirmation before entering the workspace.
- Existing-workspace destination after a later login.
- Session loading, field validation, API errors, sign out, and protected routes.
- Project creation with an editable project URL slug.
- Workspace project index with empty, loading, error, and populated states.
- Project topology home with a real PostgreSQL creation flow, resource cards,
  resource workspace sheet, add menu, zoom/history controls, environment
  context, responsive navigation, theme toggles, logs placeholder, and
  transient feedback.
- Resource workspace sections for Deployments, Database, Backups, Variables,
  Metrics, Console, and Settings. The Database section reads and mutates the
  selected project's dedicated PostgreSQL instance through typed API calls;
  Metrics reads live per-project runtime data through the same boundary.
- Light and dark dashboard themes persisted as local view preferences.
- Cloudflare Workers static-asset deployment with SPA fallback.

### Not in the current slice

- Password reset or email delivery.
- OAuth or social login.
- Multiple workspaces per user.
- Workspace switching, invitations, billing data, or settings mutations.
- Redis/app-service provisioning, deployment execution, log data, backups, and
  persistent topology editing.

The topology home now owns one real PostgreSQL resource per project. Database
tables, rows, schema creation, SQL results, live stats, runtime metrics, and
safe configuration settings come from that instance. Other resource types and
operational sections keep their visual shell but remain explicitly unavailable
until their APIs and product contracts exist. New UI must not imply that an
omitted capability exists.

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
| `/`                                              | Any           | Session destination                           | Anonymous → `/login`; authenticated without workspace → `/new/workspace`; authenticated with workspace → `/workspace/:slug` |
| `/login`                                         | Public only   | Sign-in form                                  | Authenticated users are sent to their workspace destination                                                                 |
| `/register`                                      | Public only   | Registration form                             | Authenticated users are sent to their workspace destination                                                                 |
| `/new/workspace`                                 | Authenticated | First-workspace form or creation confirmation | An account that already has a workspace is sent to `/workspace/:slug`                                                       |
| `/workspace/:slug`                               | Authenticated | Workspace project index                       | No workspace → `/new/workspace`; a non-matching slug → the account's own workspace                                          |
| `/workspace/:workspaceSlug/project/:projectSlug` | Authenticated | Project topology home                         | Workspace mismatch → the account's own workspace; missing project → unavailable state                                       |
| Any other route                                  | Any           | None                                          | Redirect to `/`                                                                                                             |

The route guards live in `apps/web/src/App.tsx`. The workspace page also
protects against revisiting the creation route after the account already has a
workspace.

### Navigation model

Navigation is intentionally small at the workspace boundary:

- Brand mark identifies the product on auth/onboarding screens.
- Auth screens link only to the alternate auth mode.
- Workspace project index exposes the signed-in email, project list, `New
project`, and `Sign out`.
- Project home provides the topology rail, workspace/environment context, and a
  clear route back to the workspace project index.
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
  → workspace name + generated slug
  → POST /workspaces
  → "Workspace created" confirmation
  → Continue to workspace
  → /workspace/:slug
```

Registration signs the user in immediately. Because the account has no
workspace, the returned `workspace` value is `null` and the router selects
`/new/workspace`.

### Returning user with a workspace

```text
Open / or /login
  → POST /auth/login
  → session response includes workspace
  → /workspace/:slug
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

### Create a project and open topology home

```text
/workspace/:slug
  → GET /workspaces/:slug/projects
  → empty project state or project list
  → New project
  → project name + generated slug
  → POST /workspaces/:slug/projects
  → navigate to /workspace/:slug/project/:projectSlug
  → GET /workspaces/:slug/projects/:projectSlug
  → topology dashboard
  → Add → Postgres
  → database display name
  → POST /workspaces/:slug/projects/:projectSlug/resources
  → dedicated PostgreSQL instance + own volume + login role + connectivity check
  → ready Postgres card + resource workspace
```

Project creation is scoped to the current workspace. The API owns slug
normalization and uniqueness; the client mirrors the normalization for fast
feedback. PostgreSQL creation is scoped to the project and is idempotent: a
retry resumes a `provisioning`/`error` resource, while a ready resource is
returned without creating a second database.

## 5. Screen specifications

### 5.1 Authentication shell

Component: `AuthShell`  
Used by: `/login`, `/register`

#### Desktop composition

- Full viewport white canvas with `1rem` outer padding; `2rem` from the
  `md` breakpoint upward.
- Centered frame with `max-width: 1200px`.
- Two-column grid at `lg`:
  - brand panel: `0.82fr`;
  - form panel: `1.18fr`.
- Frame has a subtle border, `1rem` radius, and a low-contrast shadow.
- Brand panel has a minimum viewport-height and contains:
  - `Knotree Cloud` brand mark;
  - headline: “A calmer way to build together”;
  - supporting copy: “Knotree Cloud gives teams a flexible workspace to turn
    ideas into structure.”;
  - decorative line-art motif;
  - footer phrase: “Ideas. Structure. Progress together.”
- Form panel centers a content column no wider than `25rem`.

#### Mobile composition

- The frame collapses to one column.
- The brand panel becomes a compact top section with a minimum height of
  `18rem`; the form follows below.
- Form padding is `1.5rem` on small screens and increases at `md`/`lg`.
- Decorative imagery is clipped and never becomes interactive or required to
  understand the form.

### 5.2 Login

Heading: `Welcome back`  
Supporting text: `Sign in to your workspace`

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

Layout:

- Full viewport white canvas.
- Centered content frame no wider than `38rem`.
- Brand mark at the top.
- Two-segment stepper with accessible label `Step 1 of 2`.
- Content column no wider than `25rem`.

Heading: `Create your first workspace`  
Supporting text: `A workspace is where your ideas come together.`

Fields:

| Field          | Behavior                                                                                                               |
| -------------- | ---------------------------------------------------------------------------------------------------------------------- |
| Workspace name | Required, 1–80 characters; changing it auto-generates a slug until the user edits the slug manually                    |
| Workspace URL  | Lowercase editable slug; shows the prefix `cloud.knotree.com/workspace/`; must be kebab-case and at most 48 characters |

CTA: `Create workspace`  
Helper text: `You can update these details later.`

The slug generation is deterministic: transliterate accents, lowercase,
replace runs of non-alphanumeric characters with separators, and limit the
result to 48 characters. The client performs immediate validation; the API
remains authoritative for uniqueness and reserved route names.

### 5.5 Creation confirmation

Displayed inline after a successful `POST /workspaces`:

- Centered checkmark in the primary color.
- Heading: `Workspace created`.
- Supporting copy: `<workspace name> is ready for your ideas.`
- Full-width CTA: `Continue to workspace`.

The confirmation is intentionally separate from the API response transition so
the user can recognize that the workspace was created before entering it.

### 5.6 Workspace project index

Route: `/workspace/:slug`

This is the authenticated project index and the entry point for project work.

- Header contains compact brand mark on the left.
- Signed-in email is visible on desktop and hidden on narrow screens to keep
  the header compact.
- Header provides `New project` and `Sign out` actions.
- Empty state uses the heading `Create your first project`, explains that a
  project owns topology/resources, and keeps `Create project` as the dominant
  action.
- Populated state lists projects with name, slug, count, and links to their
  topology homes.
- Loading uses a centered spinner; project-list failures use a page-level
  alert while leaving the create action available.

The workspace route must only render the workspace returned for the current
session. A mismatching URL slug is corrected by the route guard.

### 5.7 Project topology home

Component: `ProjectHomePage` and `TopologyDashboard`
Route: `/workspace/:workspaceSlug/project/:projectSlug`

The topology home follows the visual language in
`design/infra-topology-dashboard.html`: a quiet light canvas by default, a
64px topbar, a 64px desktop rail, and a dotted topology work area. A dark
theme uses the same layout and replaces the page-local surface tokens.

#### Desktop composition

- Topbar: Knotree mark, workspace switcher, environment switcher, activity,
  notifications, theme toggle, billing-plan badge, and Agent affordance.
- Side rail: Topology (active), Metrics, Logs, Resources, Settings, and the
  account/sign-out control.
- Canvas: blue `Add` button, the project service card, an optional real
  PostgreSQL card, a dashed connector when the database exists, and
  zoom/history/layers controls.
- Resource cards expose name, status, and database name. Clicking a card opens
  the `ResourceWorkspace` sheet. The sheet has `Deployments`, `Database`,
  `Backups`, `Variables`, `Metrics`, `Console`, and `Settings` tabs.
- The `Deployments` tab communicates resource status and lifecycle history.
  The `Database` tab has `Data`, `Stats`, and `Config` sub-tabs, search,
  refresh, live table rows, a table builder, bounded SQL execution, and a
  truthful `No tables yet` state when the dedicated instance has no schema
  data. `Connect` copies the real connection string without displaying
  credentials in the UI.
- The `Metrics` tab calls the selected resource's metrics endpoint, keeps live
  polling at the server-provided sample interval, and renders CPU, memory,
  volume, network RX/TX, and disk read/write series. It displays current
  values, the last update time, loading/error states, and a Live/Paused control;
  its range selector loads 1-hour, 6-hour, 24-hour, 7-day, or 30-day history,
  and hovering any chart shows the nearest sample's timestamp and exact values.
  It never uses placeholder chart coordinates.
- Add → `Postgres` opens the creation dialog. The API generates the
  database identifier, login role, and strong password, provisions a dedicated
  per-project instance, and returns a connection string after a successful
  connectivity check.
- `Backups`, `Console`, and write-oriented `Settings` controls use explicit
  empty or unavailable states until their APIs exist. Redis and App service
  remain explicit coming-soon choices and do not create fake nodes.

#### Context and transient state

- Workspace and environment controls are keyboard-operable menus.
- Environment options are `production` and `staging`; selection is local to
  the current page.
- Zoom is clamped from 80% to 125%; fit returns to 100%. Zoom and selected
  resource are persisted per project in local storage as view preferences.
- The global dashboard theme and canvas theme toggle between `light` and
  `dark`, and persist in local storage. Theme controls have explicit labels.
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
  show a safe, page-level message; raw database errors never reach the browser.
- Activity, notifications, Agent, Resources, Settings, undo/redo, and
  layers provide explicit placeholder feedback until their APIs exist.
- Direct project lookup failures show `Project unavailable` and a `Back to
workspace` action.

#### Mobile composition

- The desktop rail becomes a fixed bottom navigation bar.
- Topbar keeps a truncated workspace label, environment, activity, and
  notifications, and the theme action; Agent and the billing badge hide to
  preserve space.
- The dotted canvas stacks resource cards in a single column; connectors are
  hidden because relationship lines are not useful in the narrow layout.
- `ResourceWorkspace` becomes a full-viewport sheet with a horizontally
  scrollable tab bar; the body remains vertically scrollable and the fixed
  bottom navigation is not visible while the modal is open.
- The layout must remain within the viewport width at 320px and 390px.

The project card is a logical project node. PostgreSQL topology data comes only
from the typed resource API; a missing resource is represented by the empty
state `Create your first database`, not by a fabricated online card.

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

The visual direction is “quiet structure”: an airy white foundation, deep pine
for action, indigo for links, and muted slate for supporting information.
Avoid gradients, heavy illustrations, glassmorphism, and dense dashboard
chrome in this product area.

### 6.2 Color tokens

Tokens are defined in `apps/web/src/index.css` using OKLCH so components use
semantic roles instead of hard-coded colors.

| Token                | Current value            | Role                                |
| -------------------- | ------------------------ | ----------------------------------- |
| `background`         | `oklch(1 0 0)`           | Page and input background           |
| `foreground`         | `oklch(0.19 0.025 255)`  | Primary text                        |
| `primary`            | `oklch(0.31 0.075 165)`  | Main CTA, success mark, active step |
| `primary-foreground` | `oklch(0.985 0.01 165)`  | Text/icon on primary                |
| `secondary`          | `oklch(0.965 0.012 255)` | Secondary surfaces                  |
| `muted`              | `oklch(0.972 0.008 255)` | Hover and quiet surfaces            |
| `muted-foreground`   | `oklch(0.52 0.025 255)`  | Supporting text                     |
| `accent`             | `oklch(0.965 0.02 260)`  | Accent surface                      |
| `link`               | `oklch(0.42 0.13 275)`   | Text links                          |
| `destructive`        | `oklch(0.58 0.19 26)`    | Validation and API error            |
| `border`             | `oklch(0.90 0.018 255)`  | Dividers and frames                 |
| `input`              | `oklch(0.86 0.025 255)`  | Input border                        |
| `ring`               | `oklch(0.45 0.09 165)`   | Keyboard focus ring                 |

Rules:

- Prefer semantic tokens (`bg-primary`, `text-muted-foreground`,
  `border-border`) over raw colors.
- Do not use color as the only error or status signal; pair it with text,
  structure, or an icon.
- Preserve the contrast of primary text, field errors, and focus rings against
  white surfaces.

### 6.3 Typography

- Font family: Geist Variable, loaded from `@fontsource-variable/geist`.
- `font-heading` and `font-sans` resolve to Geist Variable.
- Headings use semibold weight and tight tracking, usually `text-3xl` to
  `text-5xl` depending on viewport.
- Body copy uses regular weight, `text-base`, and approximately `1.75` line
  height for supporting paragraphs.
- Eyebrows use small text, medium weight, uppercase, and increased tracking.
- Error text is at least `text-sm` and remains readable when zoomed.

### 6.4 Spacing, shape, and elevation

- Base radius token: `0.75rem`.
- Small/medium/large radii derive from the base token.
- Inputs and buttons are compact but touchable; primary form buttons use
  `h-11` in the auth/onboarding forms.
- Form groups use about `1.25rem` between fields and `1.5rem` around the
  submit region.
- The auth frame uses a single subtle shadow rather than layered elevation.
- Success marks are circular and `3.25rem` in the workspace surfaces.

### 6.5 Iconography and imagery

- Use `lucide-react` icons for controls and status affordances.
- Current icons: eye/eye-off for password visibility, check for success, and
  log-out for sign out.
- Decorative assets:
  - `apps/web/public/brand/knotree-mark.png` — brand mark;
  - `apps/web/public/brand/knotree-lines.png` — non-semantic auth motif.
- Decorative images use an empty `alt` attribute. Product-critical information
  must remain in text.

### 6.6 Topology dashboard tokens

The topology dashboard is a denser product surface than auth/onboarding, but
it keeps the same restraint. Its page-local tokens live in
`apps/web/src/pages/project-home.css`:

| Token               | Value     | Role                                |
| ------------------- | --------- | ----------------------------------- |
| `--project-bg`      | `#ffffff` | Canvas and card background          |
| `--project-fg`      | `#111111` | Node titles and primary controls    |
| `--project-accent`  | `#1677ff` | Add CTA, selected node, active rail |
| `--project-surface` | `#f7f8fa` | Card footers, hover, quiet surfaces |
| `--project-muted`   | `#6b7280` | Supporting text and connector lines |
| `--project-border`  | `#d9dee7` | Shell, card, and control borders    |
| `--project-ok`      | `#16803c` | Ready state text and status accents |
| `--project-warn`    | `#b45309` | Pending and caution states          |

The default light values above are overridden by the `[data-theme="dark"]`
dashboard scope (`#0f1216` background, `#f4f6f8` foreground, `#171c22`
surface, and `#2a323c` border). Topology layout constants are a 64px topbar,
a 64px desktop rail, 8px shell inset, 8px card radius, and 360px desktop
node cards. The canvas uses a 24px dot grid, low-elevation card shadows, and
dashed connector paths. The resource workspace is at most 1180x860px on
desktop. On mobile the rail is 60px high, the content is stacked, and the
resource workspace fills the viewport.

Use the local topology tokens only inside the topology page. Shared auth and
workspace surfaces continue to use the semantic OKLCH tokens above.

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
| `AuthShell`                                                  | Shared auth composition and brand panel                                          | Use for public auth entry screens only                                         |
| `BrandMark`                                                  | Product identity, compact or full size                                           | Keep `alt=""` because adjacent text carries the name                           |
| `PasswordField`                                              | Password input plus visibility toggle                                            | Always provide correct `autocomplete` and field error                          |
| `LoadingScreen`                                              | Auth bootstrap loading state                                                     | Use before protected/public route decisions are known                          |
| `AuthPage`                                                   | Login/register form and local validation                                         | Mode is explicit: `login` or `register`                                        |
| `NewWorkspacePage`                                           | First workspace form and success state                                           | Must not become a general workspace CRUD screen without a new contract         |
| `WorkspacePage`                                              | Project index, empty state, project list, and sign out                           | Keep workspace-level project selection here                                    |
| `ProjectCreateDialog`                                        | Create and validate a project name and slug                                      | Use for project creation; API remains authoritative                            |
| `PostgresCreateDialog`                                       | Request one real PostgreSQL resource and show pending/error states               | Never collect or persist database passwords in the browser                     |
| `ProjectHomePage`                                            | Load one project and render unavailable/loading states                           | Keep route data fetching typed and scoped to the current workspace             |
| `TopologyDashboard`                                          | Topbar, rail, topology canvas, controls, menus, themes, and feedback             | Keep unsupported resource actions explicit until their provisioning APIs exist |
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
| Global theme and canvas theme                                         | `TopologyDashboard`                     | `localStorage` keys `project-topology-dashboard-theme` and `project-topology-canvas-theme`          |
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
| `GET`  | `/workspaces/:slug`                                          | Session                     | Reserved for future workspace data loading         |
| `GET`  | `/workspaces/:workspaceSlug/projects`                        | Session + membership        | List projects in the current workspace             |
| `POST` | `/workspaces/:workspaceSlug/projects`                        | Session + membership + CSRF | Create a project                                   |
| `GET`  | `/workspaces/:workspaceSlug/projects/:projectSlug`           | Session + membership        | Load one project for topology home                 |
| `GET`  | `/workspaces/:workspaceSlug/projects/:projectSlug/resources` | Session + membership        | Load persisted PostgreSQL resources                |
| `POST` | `/workspaces/:workspaceSlug/projects/:projectSlug/resources` | Session + membership + CSRF | Provision or retry the project PostgreSQL resource |
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
    "name": "Acme Studio",
    "slug": "acme-studio"
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

`POST /workspaces/:workspaceSlug/projects` accepts `{ "name": "...", "slug":
"..." }`. The slug is normalized server-side and is unique within the
workspace. A duplicate returns `PROJECT_SLUG_TAKEN`; an inaccessible workspace
or project returns the corresponding not-found envelope.

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

`POST /workspaces/:workspaceSlug/projects/:projectSlug/resources` accepts
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
`SLUG_TAKEN`, `WORKSPACE_EXISTS`, `AUTHENTICATION_REQUIRED`, and
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
| Workspace    | Invalid slug                                | Field error for lowercase URL slug                                            |
| Workspace    | Slug already used                           | API field error/page alert for `SLUG_TAKEN`                                   |
| Workspace    | Second creation attempt                     | API conflict `WORKSPACE_EXISTS`; route normally redirects existing users away |
| Project      | Empty/too-long name                         | Field error for project name                                                  |
| Project      | Invalid slug                                | Field error for lowercase project URL slug                                    |
| Project      | Slug already used in workspace              | API conflict `PROJECT_SLUG_TAKEN` mapped to the slug field/page alert         |
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
  never trust a slug supplied by the user as proof of membership.
- Project list/create/detail requests are scoped through the authenticated
  workspace membership; the route slug is never treated as authorization.

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
`/workspace/:slug`, and `/workspace/:workspaceSlug/project/:projectSlug` resolve
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
- First workspace creation routes to `/workspace/:slug` after confirmation.
- Existing workspace login routes directly to that workspace.
- Slug normalization handles accents and kebab-case rules.
- Project creation routes to `/workspace/:workspaceSlug/project/:projectSlug`.
- Project list/detail API responses drive the workspace list and topology home.
- Topology interactions cover resource selection/workspace, Add menu feedback,
  workspace tabs, connection copy, theme/log navigation, zoom controls,
  responsive bottom navigation, and no-overflow mobile layout.
- The Metrics tab loads and renders live CPU, memory, volume, network, and disk
  read/write data for a ready PostgreSQL resource, exposes the live polling and
  range controls, and shows hover details for historical samples.

Every new route or meaningful interaction should add:

1. a route/interaction test;
2. an error and pending-state assertion;
3. a responsive/accessibility check;
4. a production build check when environment or routing changes.

### Manual visual QA checklist

- Check 320px, 390px, 768px, 1024px, and desktop widths.
- Confirm no horizontal overflow in the workspace URL input.
- Tab through every form without a mouse.
- Toggle password visibility without submitting the form.
- Verify errors are readable and do not shift the primary CTA unpredictably.
- Refresh `/new/workspace` and `/workspace/:slug` while authenticated.
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
| Project topology home            | `apps/web/src/pages/project-home-page.tsx`, `apps/web/src/pages/project-home.css`                          |
| Product composition              | `apps/web/src/components/`                                                                                 |
| Design tokens and layout CSS     | `apps/web/src/index.css`                                                                                   |
| Frontend deployment              | `apps/web/wrangler.jsonc`                                                                                  |
| Browser-facing API contract      | `apps/api/src/auth.rs`, `apps/api/src/workspaces.rs`, `apps/api/src/projects.rs`, `apps/api/src/models.rs` |
