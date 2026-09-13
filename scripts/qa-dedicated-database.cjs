const { chromium } = require('playwright')
const { execFileSync } = require('node:child_process')

const webUrl = process.env.QA_WEB_URL || 'http://localhost:5175'
const email = `qa-${Date.now()}@example.com`
const workspaceName = `QA Dedicated ${Date.now()}`
let resource = null

async function main() {
  const browser = await chromium.launch({ headless: true })
  try {
    const context = await browser.newContext()
    const page = await context.newPage()
    const errors = []
    page.on('console', (message) => {
      if (message.type() === 'error') errors.push(message.text())
    })

  await page.goto(`${webUrl}/register`, { waitUntil: 'networkidle' })
  await page.getByLabel('Full name').fill('Dedicated Database QA')
  await page.getByLabel('Work email').fill(email)
  await page.getByRole('textbox', { name: 'Password' }).fill('correct horse battery staple')
  await page.getByRole('button', { name: 'Create account' }).click()
  await page.waitForURL('**/new/workspace')
  await page.getByLabel('Workspace name').fill(workspaceName)
  await page.getByRole('button', { name: 'Create workspace' }).click()
  await page.getByRole('button', { name: 'Continue to workspace' }).click()
  await page.waitForURL('**/workspace/**')
  await page.getByRole('button', { name: 'New project' }).first().click()
  await page.getByLabel('Project name').fill('Dedicated PostgreSQL')
  await page.getByRole('dialog').getByRole('button', { name: 'Create project', exact: true }).click()
  await page.waitForURL('**/project/dedicated-postgresql')
  await page.getByText('Create your first database').waitFor()

  await page.getByRole('button', { name: 'Add' }).click()
  await page.getByRole('button', { name: 'Postgres', exact: true }).click()
  await page.getByRole('dialog').getByLabel('Database name').fill('QA Dedicated DB')
  const createResponsePromise = page.waitForResponse((response) =>
    response.url().endsWith('/resources') && response.request().method() === 'POST'
  )
  await page.getByRole('dialog').getByRole('button', { name: 'Create database', exact: true }).click()
  resource = await (await createResponsePromise).json()
  if (resource.status !== 'ready' || resource.clusterProvider !== 'docker') {
    throw new Error(`resource was not ready on docker: ${JSON.stringify(resource)}`)
  }
  if (!resource.clusterName) throw new Error('resource did not return a dedicated cluster name')

  const resourceDialog = page.getByRole('dialog').first()
  await page.getByRole('button', { name: 'QA Dedicated DB resource, online' }).waitFor()
  await resourceDialog.getByRole('tab', { name: 'Database' }).click()
  await resourceDialog.getByRole('button', { name: 'New Table' }).click()
  const tableDialog = page.getByRole('dialog').last()
  await tableDialog.getByLabel('Table name').fill('qa_records')
  await tableDialog.getByRole('button', { name: 'Add column' }).click()
  await tableDialog.getByLabel('Column 2 name').fill('label')
  await tableDialog.getByLabel('Column 2 type').selectOption('text')
  await tableDialog.getByRole('button', { name: 'Create table' }).click()
  await resourceDialog.getByRole('button', { name: /qa_records/ }).waitFor()

  await resourceDialog.getByRole('button', { name: /qa_records/ }).click()
  await resourceDialog.getByRole('columnheader', { name: /id/ }).waitFor()
  await resourceDialog.getByLabel('SQL query').fill("INSERT INTO public.qa_records (id, label) VALUES (1, 'ready')")
  await resourceDialog.getByRole('button', { name: 'Run query' }).click()
  await resourceDialog.getByRole('status').filter({ hasText: 'Query completed' }).waitFor()
  await resourceDialog.getByLabel('SQL query').fill('SELECT * FROM public.qa_records')
  await resourceDialog.getByRole('button', { name: 'Run query' }).click()
  await resourceDialog.getByText('ready').waitFor()

  await resourceDialog.getByRole('tab', { name: 'Stats' }).click()
  await resourceDialog.getByText('Connections').waitFor()
  await resourceDialog.getByRole('tab', { name: 'Config' }).click()
  await resourceDialog.getByText('server_version').waitFor()

    const inspected = execFileSync('docker', [
    'inspect',
    '--format={{index .Config.Labels "com.knotree.project-id"}}|{{.State.Running}}|{{range .Mounts}}{{.Name}}{{end}}',
    resource.clusterName,
    ], { encoding: 'utf8' }).trim()
    const [projectId, running, volume] = inspected.split('|')
    if (running !== 'true' || projectId.length !== 36 || !volume.startsWith('knotree-pg-data-')) {
      throw new Error(`dedicated container inspection failed: ${inspected}`)
    }

    console.log(JSON.stringify({
    ok: true,
    resource: {
      id: resource.id,
      status: resource.status,
      clusterProvider: resource.clusterProvider,
      clusterName: resource.clusterName,
      host: resource.host,
      port: resource.port,
    },
    createdTable: 'qa_records',
    queryRoundTrip: true,
    statsAndConfig: true,
    dedicatedContainer: { projectId, running, volume },
    consoleErrors: errors,
  }))
  } finally {
    await browser.close()
  }
}

function cleanup() {
  if (resource?.clusterName) {
    try { execFileSync('docker', ['rm', '--force', resource.clusterName], { stdio: 'ignore' }) } catch {}
    try { execFileSync('docker', ['volume', 'rm', `knotree-pg-data-${resource.clusterName.slice('knotree-pg-'.length)}`], { stdio: 'ignore' }) } catch {}
  }
  try {
    const escapedEmail = email.replaceAll("'", "''")
    execFileSync('docker', [
      'exec', 'cloud-postgres-1', 'psql', '-U', 'postgres', '-d', 'knotree_cloud',
      '--command', `DELETE FROM users WHERE email = '${escapedEmail}';`,
    ], { stdio: 'ignore' })
  } catch {}
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
}).finally(cleanup)
