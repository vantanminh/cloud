import { useEffect, useState } from "react"

import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { ApiError } from "@/lib/api"
import {
  getHtmlPageAnalytics,
  getHtmlPageIndex,
  updateHtmlPage,
} from "@/lib/resources"
import type { AppService, HtmlAnalyticsSummary } from "@/lib/types"
import { isHtmlPage } from "@/lib/types"

export function HtmlSourceEditor({
  appService,
  workspaceId,
  projectSlug,
  onToast,
  onAppServiceUpdated,
}: {
  appService: AppService
  workspaceId: string
  projectSlug: string
  onToast: (message: string) => void
  onAppServiceUpdated?: (resource: AppService) => void
}) {
  const [indexHtml, setIndexHtml] = useState("")
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    setLoading(true)
    void getHtmlPageIndex(workspaceId, projectSlug, appService.id)
      .then((payload) => {
        if (active) {
          setIndexHtml(payload.indexHtml)
          setError(null)
        }
      })
      .catch((caught) => {
        if (active) {
          setError(
            caught instanceof ApiError
              ? caught.message
              : "The HTML file could not be loaded."
          )
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false)
        }
      })
    return () => {
      active = false
    }
  }, [appService.id, projectSlug, workspaceId])

  if (appService.imageSource !== "html") {
    return (
      <article className="resource-workspace-setting-section">
        <h3>HTML source</h3>
        <p>
          This page is published from GitHub
          {appService.htmlRepo ? ` (${appService.htmlRepo})` : ""}. New pushes
          redeploy automatically when auto updates are enabled.
        </p>
        <dl>
          <div>
            <dt>Repository</dt>
            <dd>{appService.htmlRepo ?? "Unknown"}</dd>
          </div>
          <div>
            <dt>Branch</dt>
            <dd>{appService.htmlBranch ?? "default"}</dd>
          </div>
          <div>
            <dt>Commit</dt>
            <dd>{appService.htmlSha ?? "Not recorded yet"}</dd>
          </div>
        </dl>
      </article>
    )
  }

  async function handleSave() {
    setSaving(true)
    setError(null)
    try {
      const resource = await updateHtmlPage(
        workspaceId,
        projectSlug,
        appService.id,
        { indexHtml }
      )
      onAppServiceUpdated?.(resource)
      onToast("HTML page redeploy started.")
    } catch (caught) {
      setError(
        caught instanceof ApiError
          ? caught.message
          : "The HTML page could not be updated."
      )
    } finally {
      setSaving(false)
    }
  }

  return (
    <article className="resource-workspace-setting-section">
      <h3>Edit HTML</h3>
      <p>
        Update index.html and deploy a new version. Knotree keeps the injected
        analytics script and Cloudflare cache headers.
      </p>
      {loading ? (
        <p>Loading index.html…</p>
      ) : (
        <Textarea
          aria-label="index.html"
          value={indexHtml}
          rows={16}
          spellCheck={false}
          disabled={saving}
          onChange={(event) => setIndexHtml(event.target.value)}
        />
      )}
      {error ? (
        <p className="resource-workspace-app-port-error" role="alert">
          {error}
        </p>
      ) : null}
      <Button
        type="button"
        size="sm"
        disabled={loading || saving}
        onClick={() => void handleSave()}
      >
        {saving ? "Deploying…" : "Save and deploy"}
      </Button>
    </article>
  )
}

export function HtmlAnalyticsPane({
  appService,
  workspaceId,
  projectSlug,
}: {
  appService: AppService
  workspaceId: string
  projectSlug: string
}) {
  const [summary, setSummary] = useState<HtmlAnalyticsSummary | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    if (!isHtmlPage(appService)) {
      return undefined
    }
    let active = true
    setLoading(true)
    void getHtmlPageAnalytics(workspaceId, projectSlug, appService.id)
      .then((next) => {
        if (active) {
          setSummary(next)
          setError(null)
        }
      })
      .catch((caught) => {
        if (active) {
          setError(
            caught instanceof ApiError
              ? caught.message
              : "HTML analytics could not be loaded."
          )
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false)
        }
      })
    return () => {
      active = false
    }
  }, [appService, projectSlug, workspaceId])

  if (!isHtmlPage(appService)) {
    return (
      <p className="resource-workspace-muted">
        Analytics from the injected Knotree script are available for HTML pages.
      </p>
    )
  }

  if (loading) {
    return <p>Loading HTML analytics…</p>
  }
  if (error) {
    return (
      <p className="resource-workspace-app-port-error" role="alert">
        {error}
      </p>
    )
  }
  if (!summary) {
    return <p>No analytics yet.</p>
  }

  return (
    <div className="html-analytics-grid">
      <article className="resource-workspace-setting-section">
        <h3>Page analytics</h3>
        <p>
          Collected from the script Knotree injects into this site: pageviews,
          sessions, duration, referrers, browsers, clicks, and errors.
        </p>
        <dl>
          <div>
            <dt>Pageviews</dt>
            <dd>{summary.pageviews}</dd>
          </div>
          <div>
            <dt>Sessions</dt>
            <dd>{summary.sessions}</dd>
          </div>
          <div>
            <dt>Avg duration</dt>
            <dd>{Math.round(summary.avgDurationMs)} ms</dd>
          </div>
        </dl>
      </article>
      <CountList title="Top paths" rows={summary.topPaths} />
      <CountList title="Referrers" rows={summary.topReferrers} />
      <CountList title="Browsers" rows={summary.browsers} />
      <CountList title="Events" rows={summary.eventTypes} />
      <article className="resource-workspace-setting-section">
        <h3>Recent events</h3>
        <ul className="html-analytics-recent">
          {summary.recent.length === 0 ? (
            <li>No events recorded yet. Open the public page to generate traffic.</li>
          ) : (
            summary.recent.map((event, index) => (
              <li key={`${event.occurredAt}-${index}`}>
                <strong>{event.eventType}</strong> {event.path}
                {event.referrer ? ` · ${event.referrer}` : ""}
              </li>
            ))
          )}
        </ul>
      </article>
    </div>
  )
}

function CountList({
  title,
  rows,
}: {
  title: string
  rows: Array<{ name: string; count: number }>
}) {
  return (
    <article className="resource-workspace-setting-section">
      <h3>{title}</h3>
      {rows.length === 0 ? (
        <p className="resource-workspace-muted">No data yet.</p>
      ) : (
        <dl>
          {rows.map((row) => (
            <div key={row.name}>
              <dt>{row.name}</dt>
              <dd>{row.count}</dd>
            </div>
          ))}
        </dl>
      )}
    </article>
  )
}
