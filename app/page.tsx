'use client'

import { useState, useEffect, useCallback, ReactNode } from 'react'

// --- Types ---

interface Source {
  id: number
  name: string
  caldav_url: string
  username: string
  ics_path: string
  sync_interval_secs: number
  last_synced: string | null
  last_sync_status: string | null
  last_sync_error: string | null
  created_at: string
}

interface Destination {
  id: number
  name: string
  ics_url: string
  caldav_url: string
  calendar_name: string
  username: string
  sync_interval_secs: number
  sync_all: boolean
  keep_local: boolean
  last_synced: string | null
  last_sync_status: string | null
  last_sync_error: string | null
  created_at: string
}

interface HealthStatus {
  status: string
  uptime_seconds?: number
  source_count?: number
  db_ok?: boolean
}

type Tab = 'sources' | 'destinations'

// --- Form defaults ---

type IntervalUnit = 'seconds' | 'minutes' | 'hours'

const emptySrcForm = {
  name: '',
  caldav_url: '',
  username: '',
  password: '',
  ics_path: '',
  sync_interval_value: 60,
  sync_interval_unit: 'minutes' as IntervalUnit,
}

const emptyDestForm = {
  name: '',
  ics_url: '',
  caldav_url: '',
  calendar_name: '',
  username: '',
  password: '',
  sync_interval_value: 60,
  sync_interval_unit: 'minutes' as IntervalUnit,
  sync_all: false,
  keep_local: false,
}

function toSecs(value: number, unit: IntervalUnit): number {
  if (unit === 'hours') return value * 3600
  if (unit === 'minutes') return value * 60
  return value
}

function fromSecs(secs: number): { value: number; unit: IntervalUnit } {
  if (secs === 0) return { value: 0, unit: 'seconds' }
  if (secs % 3600 === 0) return { value: secs / 3600, unit: 'hours' }
  if (secs % 60 === 0) return { value: secs / 60, unit: 'minutes' }
  return { value: secs, unit: 'seconds' }
}

function formatInterval(secs: number): string {
  if (secs === 0) return 'Manual only'
  const { value, unit } = fromSecs(secs)
  return `${value} ${unit}`
}

// --- Helpers ---

function formatUptime(secs: number): string {
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  if (h > 0) return `${h}h ${m}m`
  return `${m}m`
}

function formatTime(iso: string | null): string {
  if (!iso) return 'Never'
  return new Date(iso + 'Z').toLocaleString()
}

function statusDot(status: string | null, error: string | null) {
  if (!status) return <span className="sync-dot pending" title="Not synced yet" />
  if (status === 'ok') return <span className="sync-dot ok" title="Last sync successful" />
  return <span className="sync-dot error" title={error ?? 'Sync failed'} />
}

// --- Shared: SyncItemList ---

interface DetailRow {
  label: string
  value: ReactNode
}

interface SyncItem {
  id: number
  name: string
  last_sync_status: string | null
  last_sync_error: string | null
}

interface SyncItemListProps<T extends SyncItem> {
  title: string
  addButtonText: string
  items: T[]
  emptyMessage: string
  expandedIds: Set<number>
  onToggle: (id: number) => void
  syncingMap: Record<string, boolean>
  syncKeyPrefix: string
  onAdd: () => void
  onSync: (id: number) => void
  onEdit: (item: T) => void
  onDelete: (item: T) => void
  getSubtitle: (item: T) => string
  getDetails: (item: T) => DetailRow[]
  renderExtraPanel?: (item: T) => ReactNode
}

function SyncItemList<T extends SyncItem>({
  title,
  addButtonText,
  items,
  emptyMessage,
  expandedIds,
  onToggle,
  syncingMap,
  syncKeyPrefix,
  onAdd,
  onSync,
  onEdit,
  onDelete,
  getSubtitle,
  getDetails,
  renderExtraPanel,
}: SyncItemListProps<T>) {
  return (
    <>
      <div className="section-header">
        <h2>{title}</h2>
        <button className="app-btn app-btn-primary" onClick={onAdd}>
          <i className="icons10-plus" />
          <span>{addButtonText}</span>
        </button>
      </div>

      {items.length === 0 ? (
        <div className="empty-state">{emptyMessage}</div>
      ) : (
        <div className="app-accordion">
          {items.map(item => {
            const open = expandedIds.has(item.id)
            const isSyncing = syncingMap[`${syncKeyPrefix}-${item.id}`]
            return (
              <div className="accordion-row" key={item.id}>
                <div className="app-accordion-header" onClick={() => onToggle(item.id)}>
                  <span className={`chevron ${open ? 'open' : ''}`} />
                  {statusDot(item.last_sync_status, item.last_sync_error)}
                  <strong>{item.name}</strong>
                  <span className="accordion-subtitle">{getSubtitle(item)}</span>
                </div>
                <div className={`app-accordion-panel ${open ? 'show' : ''}`}>
                  {getDetails(item).map((row, i) => (
                    <div className="detail-row" key={i}>
                      <strong>{row.label}</strong>
                      <span>{row.value}</span>
                    </div>
                  ))}
                  {item.last_sync_status === 'error' && item.last_sync_error && (
                    <div className="sync-error-msg">Error: {item.last_sync_error}</div>
                  )}
                  {renderExtraPanel?.(item)}
                  <div className="accordion-actions">
                    <button
                      className="app-btn app-btn-primary"
                      onClick={() => onSync(item.id)}
                      disabled={isSyncing}
                    >
                      <i className="icons10-refresh" />
                      <span>{isSyncing ? 'Syncing...' : 'Sync Now'}</span>
                    </button>
                    <button className="app-btn app-btn-subtle" onClick={() => onEdit(item)}>
                      <i className="icons10-pencil" />
                      <span>Edit</span>
                    </button>
                    <button className="app-btn app-btn-subtle" onClick={() => onDelete(item)}>
                      <i className="icons10-trash" />
                      <span>Delete</span>
                    </button>
                  </div>
                </div>
              </div>
            )
          })}
        </div>
      )}
    </>
  )
}

// --- Shared: FormDialog ---

interface FormDialogProps {
  open: boolean
  title: string
  isEditing: boolean
  onClose: () => void
  onSubmit: (e: React.FormEvent) => void
  children: ReactNode
}

function FormDialog({ open, title, isEditing, onClose, onSubmit, children }: FormDialogProps) {
  if (!open) return null
  return (
    <div className="app-dialog show" onClick={onClose}>
      <div className="app-dialog-modal wide" onClick={e => e.stopPropagation()}>
        <div className="app-dialog-header">
          <h3>{title}</h3>
        </div>
        <div className="app-dialog-body">
          <form onSubmit={onSubmit}>
            <div className="form-grid">{children}</div>
            <div className="dialog-actions">
              <button type="button" className="app-btn app-btn-subtle" onClick={onClose}>
                Cancel
              </button>
              <button type="submit" className="app-btn app-btn-primary">
                {isEditing ? 'Update' : 'Create'}
              </button>
            </div>
          </form>
        </div>
      </div>
    </div>
  )
}

// --- Shared: DeleteDialog ---

interface DeletePrompt {
  kind: 'source' | 'dest'
  id: number
  name: string
}

interface DeleteDialogProps {
  prompt: DeletePrompt | null
  onCancel: () => void
  onConfirm: () => void
}

function DeleteDialog({ prompt, onCancel, onConfirm }: DeleteDialogProps) {
  if (!prompt) return null
  return (
    <div className="app-dialog show" onClick={onCancel}>
      <div className="app-dialog-modal" onClick={e => e.stopPropagation()}>
        <div className="app-dialog-header">
          <h3>Confirm Delete</h3>
        </div>
        <div className="app-dialog-body">
          <p>
            Delete <strong>{prompt.name}</strong>? This cannot be undone.
          </p>
          <div className="dialog-actions">
            <button className="app-btn app-btn-subtle" onClick={onCancel}>
              Cancel
            </button>
            <button className="app-btn app-btn-danger" onClick={onConfirm}>
              Delete
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}

// --- Component ---

export default function Home() {
  const [tab, setTab] = useState<Tab>('sources')

  // Data
  const [sources, setSources] = useState<Source[]>([])
  const [destinations, setDestinations] = useState<Destination[]>([])
  const [health, setHealth] = useState<HealthStatus | null>(null)

  // Flash message
  const [message, setMessage] = useState<{
    text: string
    type: 'success' | 'error'
  } | null>(null)

  // Source form
  const [srcDialogOpen, setSrcDialogOpen] = useState(false)
  const [editingSrc, setEditingSrc] = useState<Source | null>(null)
  const [srcForm, setSrcForm] = useState({ ...emptySrcForm })

  // Destination form
  const [destDialogOpen, setDestDialogOpen] = useState(false)
  const [editingDest, setEditingDest] = useState<Destination | null>(null)
  const [destForm, setDestForm] = useState({ ...emptyDestForm })

  // Accordion expansion
  const [expandedSrcs, setExpandedSrcs] = useState<Set<number>>(new Set())
  const [expandedDests, setExpandedDests] = useState<Set<number>>(new Set())

  // Sync-in-progress tracking
  const [syncing, setSyncing] = useState<Record<string, boolean>>({})

  // Delete confirmation
  const [deletePrompt, setDeletePrompt] = useState<DeletePrompt | null>(null)

  // ── Data fetching ──────────────────────────────────────────────

  const fetchSources = useCallback(async () => {
    try {
      const res = await fetch('/api/sources')
      if (res.ok) {
        const data = await res.json()
        setSources(data.sources)
      }
    } catch {
      /* network error */
    }
  }, [])

  const fetchDestinations = useCallback(async () => {
    try {
      const res = await fetch('/api/destinations')
      if (res.ok) {
        const data = await res.json()
        setDestinations(data.destinations)
      }
    } catch {
      /* network error */
    }
  }, [])

  const fetchHealth = useCallback(async () => {
    try {
      const res = await fetch('/api/health/detailed')
      setHealth(res.ok ? await res.json() : { status: 'unreachable' })
    } catch {
      setHealth({ status: 'unreachable' })
    }
  }, [])

  useEffect(() => {
    fetchSources()
    fetchDestinations()
    fetchHealth()
    const interval = setInterval(fetchHealth, 15000)
    return () => clearInterval(interval)
  }, [fetchSources, fetchDestinations, fetchHealth])

  // Auto-dismiss flash messages
  useEffect(() => {
    if (!message) return
    const t = setTimeout(() => setMessage(null), 6000)
    return () => clearTimeout(t)
  }, [message])

  // ── Generic CRUD helpers ─────────────────────────────────────

  function flash(text: string, type: 'success' | 'error') {
    setMessage({ text, type })
  }

  async function apiSubmit(url: string, method: string, body: unknown, onSuccess: () => void) {
    try {
      const res = await fetch(url, {
        method,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })
      const data = await res.json()
      flash(data.message, res.ok ? 'success' : 'error')
      if (res.ok) onSuccess()
    } catch (err) {
      flash(err instanceof Error ? err.message : 'Request failed', 'error')
    }
  }

  async function apiSync(syncKeyPrefix: string, id: number, url: string, refresh: () => void) {
    setSyncing(p => ({ ...p, [`${syncKeyPrefix}-${id}`]: true }))
    try {
      const res = await fetch(url, { method: 'POST' })
      const data = await res.json()
      flash(data.message, res.ok ? 'success' : 'error')
      refresh()
    } catch {
      flash('Sync request failed', 'error')
    } finally {
      setSyncing(p => ({ ...p, [`${syncKeyPrefix}-${id}`]: false }))
    }
  }

  async function apiDelete(url: string, refresh: () => void) {
    try {
      const res = await fetch(url, { method: 'DELETE' })
      const data = await res.json()
      flash(data.message, res.ok ? 'success' : 'error')
      refresh()
    } catch {
      flash('Delete failed', 'error')
    }
    setDeletePrompt(null)
  }

  // ── Accordion toggle ──────────────────────────────────────────

  function toggleExpanded(setter: React.Dispatch<React.SetStateAction<Set<number>>>, id: number) {
    setter(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  // ── Source handlers ────────────────────────────────────────────

  function openSrcCreate() {
    setSrcForm({ ...emptySrcForm })
    setEditingSrc(null)
    setSrcDialogOpen(true)
  }

  function openSrcEdit(src: Source) {
    const interval = fromSecs(src.sync_interval_secs)
    setSrcForm({
      name: src.name,
      caldav_url: src.caldav_url,
      username: src.username,
      password: '',
      ics_path: src.ics_path,
      sync_interval_value: interval.value,
      sync_interval_unit: interval.unit,
    })
    setEditingSrc(src)
    setSrcDialogOpen(true)
  }

  function closeSrcDialog() {
    setSrcDialogOpen(false)
    setEditingSrc(null)
  }

  async function submitSrc(e: React.FormEvent) {
    e.preventDefault()
    const url = editingSrc ? `/api/sources/${editingSrc.id}` : '/api/sources'
    const method = editingSrc ? 'PUT' : 'POST'
    const { sync_interval_value, sync_interval_unit, ...rest } = srcForm
    const body = { ...rest, sync_interval_secs: toSecs(sync_interval_value, sync_interval_unit) }
    await apiSubmit(url, method, body, () => {
      closeSrcDialog()
      fetchSources()
    })
  }

  // ── Destination handlers ───────────────────────────────────────

  function openDestCreate() {
    setDestForm({ ...emptyDestForm })
    setEditingDest(null)
    setDestDialogOpen(true)
  }

  function openDestEdit(dest: Destination) {
    const interval = fromSecs(dest.sync_interval_secs)
    setDestForm({
      name: dest.name,
      ics_url: dest.ics_url,
      caldav_url: dest.caldav_url,
      calendar_name: dest.calendar_name,
      username: dest.username,
      password: '',
      sync_interval_value: interval.value,
      sync_interval_unit: interval.unit,
      sync_all: dest.sync_all,
      keep_local: dest.keep_local,
    })
    setEditingDest(dest)
    setDestDialogOpen(true)
  }

  function closeDestDialog() {
    setDestDialogOpen(false)
    setEditingDest(null)
  }

  async function submitDest(e: React.FormEvent) {
    e.preventDefault()
    const url = editingDest ? `/api/destinations/${editingDest.id}` : '/api/destinations'
    const method = editingDest ? 'PUT' : 'POST'
    const { sync_interval_value, sync_interval_unit, ...rest } = destForm
    const body = { ...rest, sync_interval_secs: toSecs(sync_interval_value, sync_interval_unit) }
    await apiSubmit(url, method, body, () => {
      closeDestDialog()
      fetchDestinations()
    })
  }

  // ── Delete handler ─────────────────────────────────────────────

  function handleDeleteConfirm() {
    if (!deletePrompt) return
    if (deletePrompt.kind === 'source') {
      apiDelete(`/api/sources/${deletePrompt.id}`, fetchSources)
    } else {
      apiDelete(`/api/destinations/${deletePrompt.id}`, fetchDestinations)
    }
  }

  // ── Source detail definitions ──────────────────────────────────

  function getSourceDetails(src: Source): DetailRow[] {
    return [
      { label: 'CalDAV URL', value: src.caldav_url },
      { label: 'Username', value: src.username },
      {
        label: 'Sync Interval',
        value: formatInterval(src.sync_interval_secs),
      },
      { label: 'Last Synced', value: formatTime(src.last_synced) },
    ]
  }

  function renderSourceExtra(src: Source) {
    return (
      <div className="detail-row">
        <strong>ICS URL</strong>
        <span className="ics-url-row">
          <a href={`/ics/${src.ics_path}`} target="_blank" rel="noreferrer">
            {typeof window !== 'undefined'
              ? `${window.location.origin}/ics/${src.ics_path}`
              : `/ics/${src.ics_path}`}
          </a>
          <button
            className="app-btn app-btn-subtle"
            style={{ padding: '2px 8px', fontSize: 12 }}
            onClick={() =>
              navigator.clipboard.writeText(`${window.location.origin}/ics/${src.ics_path}`)
            }
          >
            <i className="icons10-copy" />
          </button>
        </span>
      </div>
    )
  }

  // ── Destination detail definitions ─────────────────────────────

  function getDestDetails(dest: Destination): DetailRow[] {
    return [
      { label: 'ICS Source', value: dest.ics_url },
      { label: 'CalDAV URL', value: dest.caldav_url },
      { label: 'Calendar', value: dest.calendar_name },
      { label: 'Username', value: dest.username },
      {
        label: 'Sync Interval',
        value: formatInterval(dest.sync_interval_secs),
      },
      {
        label: 'Sync All',
        value: dest.sync_all ? 'Yes (past + future)' : 'Future only',
      },
      {
        label: 'Keep Local',
        value: dest.keep_local ? 'Yes (preserve CalDAV events)' : 'No (mirror ICS exactly)',
      },
      { label: 'Last Synced', value: formatTime(dest.last_synced) },
    ]
  }

  // ── Render helpers ─────────────────────────────────────────────

  function healthDot() {
    if (!health) return null
    const ok = health.status === 'ok'
    return (
      <span
        className={`sync-dot ${ok ? 'ok' : 'error'}`}
        style={{ width: 6, height: 6, marginLeft: 8, marginRight: 4 }}
        title={ok ? 'Server healthy' : 'Server degraded'}
      />
    )
  }

  // ── Render ─────────────────────────────────────────────────────

  return (
    <div className="container-flex-row">
      {/* ─── Sidebar ─── */}
      <aside className="app-navbar-wrap" id="NavBarMain">
        <div className="app-navbar-header-mobile">
          <span
            className="app-navbar-toggler"
            data-win-toggle="navbar-left"
            data-win-target="#NavBarMain"
          />
          <span className="app-navbar-name">CalDAV/ICS Sync</span>
        </div>
        <nav className="app-navbar">
          <div className="app-navbar-header">
            <span
              className="app-navbar-toggler"
              data-win-toggle="navbar-left"
              data-win-target="#NavBarMain"
            />
            <span className="app-navbar-name">CalDAV/ICS Sync</span>
          </div>
          <ul className="app-navbar-list" id="app-navbar-list">
            {/* Sources tab */}
            <li className="app-navbar-list-item">
              <a
                className={tab === 'sources' ? 'active' : ''}
                href="#"
                onClick={e => {
                  e.preventDefault()
                  setTab('sources')
                }}
              >
                <i className="icons10-calendar" />
                <span>Sources ({sources.length})</span>
              </a>
            </li>

            {/* Destinations tab */}
            <li className="app-navbar-list-item">
              <a
                className={tab === 'destinations' ? 'active' : ''}
                href="#"
                onClick={e => {
                  e.preventDefault()
                  setTab('destinations')
                }}
              >
                <i className="icons10-upload" />
                <span>Destinations ({destinations.length})</span>
              </a>
            </li>

            {/* API Spec */}
            <li className="app-navbar-list-item">
              <a href="/api/openapi.json" target="_blank" rel="noreferrer">
                <i className="icons10-code-file" />
                <span>API Spec</span>
              </a>
            </li>

            {/* Server status in sidebar */}
            <li className="app-navbar-list-item" style={{ opacity: 0.7 }}>
              <a
                href="#"
                onClick={e => {
                  e.preventDefault()
                  fetchHealth()
                }}
                style={{ fontSize: 13 }}
              >
                {healthDot()}
                <span>
                  {health
                    ? health.status === 'ok'
                      ? `Up ${health.uptime_seconds != null ? formatUptime(health.uptime_seconds) : ''}`
                      : 'Degraded'
                    : 'Checking...'}
                </span>
              </a>
            </li>

            {/* Theme toggle */}
            <label className="app-navbar-theme-switch">
              <input id="app-navbar-theme-switch" type="checkbox" defaultChecked />
              <div className="app-navbar-theme-switch-icon" />
            </label>
          </ul>
        </nav>
      </aside>

      {/* ─── Main content ─── */}
      <main className="app-page-container has-padding">
        {/* Flash message */}
        {message && (
          <div
            className={`app-alert-bar ${message.type === 'success' ? 'alert-bar-success' : 'alert-bar-danger'} app-mb-10`}
          >
            <span>{message.text}</span>
            <button className="dismiss-btn" onClick={() => setMessage(null)}>
              ×
            </button>
          </div>
        )}

        {/* ─── Sources tab ─── */}
        {tab === 'sources' && (
          <SyncItemList<Source>
            title="Sources (CalDAV to ICS)"
            addButtonText="Add Source"
            items={sources}
            emptyMessage={'No sources configured. Click "Add Source" to get started.'}
            expandedIds={expandedSrcs}
            onToggle={id => toggleExpanded(setExpandedSrcs, id)}
            syncingMap={syncing}
            syncKeyPrefix="src"
            onAdd={openSrcCreate}
            onSync={id => apiSync('src', id, `/api/sources/${id}/sync`, fetchSources)}
            onEdit={openSrcEdit}
            onDelete={src => setDeletePrompt({ kind: 'source', id: src.id, name: src.name })}
            getSubtitle={src => `/ics/${src.ics_path}`}
            getDetails={getSourceDetails}
            renderExtraPanel={renderSourceExtra}
          />
        )}

        {/* ─── Destinations tab ─── */}
        {tab === 'destinations' && (
          <SyncItemList<Destination>
            title="Destinations (ICS to CalDAV)"
            addButtonText="Add Destination"
            items={destinations}
            emptyMessage={'No destinations configured. Click "Add Destination" to get started.'}
            expandedIds={expandedDests}
            onToggle={id => toggleExpanded(setExpandedDests, id)}
            syncingMap={syncing}
            syncKeyPrefix="dest"
            onAdd={openDestCreate}
            onSync={id => apiSync('dest', id, `/api/destinations/${id}/sync`, fetchDestinations)}
            onEdit={openDestEdit}
            onDelete={dest => setDeletePrompt({ kind: 'dest', id: dest.id, name: dest.name })}
            getSubtitle={dest => dest.calendar_name}
            getDetails={getDestDetails}
          />
        )}
      </main>

      {/* ─── Source form dialog ─── */}
      <FormDialog
        open={srcDialogOpen}
        title={editingSrc ? 'Edit Source' : 'New Source'}
        isEditing={!!editingSrc}
        onClose={closeSrcDialog}
        onSubmit={submitSrc}
      >
        <div className="form-field">
          <label>Name</label>
          <input
            className="app-input-text"
            type="text"
            value={srcForm.name}
            onChange={e => setSrcForm(p => ({ ...p, name: e.target.value }))}
            required
          />
        </div>
        <div className="form-field">
          <label>CalDAV URL</label>
          <input
            className="app-input-text"
            type="url"
            value={srcForm.caldav_url}
            onChange={e => setSrcForm(p => ({ ...p, caldav_url: e.target.value }))}
            required
          />
        </div>
        <div className="form-field">
          <label>Username</label>
          <input
            className="app-input-text"
            type="text"
            value={srcForm.username}
            onChange={e => setSrcForm(p => ({ ...p, username: e.target.value }))}
            required
          />
        </div>
        <div className="form-field">
          <label>
            Password
            {editingSrc ? ' (leave empty to keep current)' : ''}
          </label>
          <input
            className="app-input-text"
            type="password"
            value={srcForm.password}
            onChange={e => setSrcForm(p => ({ ...p, password: e.target.value }))}
            required={!editingSrc}
            placeholder={editingSrc ? 'Unchanged if empty' : ''}
          />
        </div>
        <div className="form-field">
          <label>ICS Path (e.g. my-calendar)</label>
          <input
            className="app-input-text"
            type="text"
            value={srcForm.ics_path}
            onChange={e => setSrcForm(p => ({ ...p, ics_path: e.target.value }))}
            required
          />
        </div>
        <div className="form-field">
          <label>Sync Interval (0 = manual only)</label>
          <div className="interval-input">
            <input
              className="app-input-text"
              type="number"
              min="0"
              value={srcForm.sync_interval_value}
              onChange={e =>
                setSrcForm(p => ({
                  ...p,
                  sync_interval_value: parseInt(e.target.value) || 0,
                }))
              }
              required
            />
            <select
              className="app-input-text"
              value={srcForm.sync_interval_unit}
              onChange={e =>
                setSrcForm(p => ({
                  ...p,
                  sync_interval_unit: e.target.value as IntervalUnit,
                }))
              }
            >
              <option value="seconds">Seconds</option>
              <option value="minutes">Minutes</option>
              <option value="hours">Hours</option>
            </select>
          </div>
        </div>
      </FormDialog>

      {/* ─── Destination form dialog ─── */}
      <FormDialog
        open={destDialogOpen}
        title={editingDest ? 'Edit Destination' : 'New Destination'}
        isEditing={!!editingDest}
        onClose={closeDestDialog}
        onSubmit={submitDest}
      >
        <div className="form-field">
          <label>Name</label>
          <input
            className="app-input-text"
            type="text"
            value={destForm.name}
            onChange={e => setDestForm(p => ({ ...p, name: e.target.value }))}
            required
          />
        </div>
        <div className="form-field">
          <label>ICS Source URL</label>
          <input
            className="app-input-text"
            type="url"
            value={destForm.ics_url}
            onChange={e => setDestForm(p => ({ ...p, ics_url: e.target.value }))}
            required
          />
        </div>
        <div className="form-field">
          <label>CalDAV Server URL</label>
          <input
            className="app-input-text"
            type="url"
            value={destForm.caldav_url}
            onChange={e => setDestForm(p => ({ ...p, caldav_url: e.target.value }))}
            required
          />
        </div>
        <div className="form-field">
          <label>Calendar Name</label>
          <input
            className="app-input-text"
            type="text"
            value={destForm.calendar_name}
            onChange={e => setDestForm(p => ({ ...p, calendar_name: e.target.value }))}
            required
          />
        </div>
        <div className="form-field">
          <label>CalDAV Username</label>
          <input
            className="app-input-text"
            type="text"
            value={destForm.username}
            onChange={e => setDestForm(p => ({ ...p, username: e.target.value }))}
            required
          />
        </div>
        <div className="form-field">
          <label>
            CalDAV Password
            {editingDest ? ' (leave empty to keep current)' : ''}
          </label>
          <input
            className="app-input-text"
            type="password"
            value={destForm.password}
            onChange={e => setDestForm(p => ({ ...p, password: e.target.value }))}
            required={!editingDest}
            placeholder={editingDest ? 'Unchanged if empty' : ''}
          />
        </div>
        <div className="form-field">
          <label>Sync Interval (0 = manual only)</label>
          <div className="interval-input">
            <input
              className="app-input-text"
              type="number"
              min="0"
              value={destForm.sync_interval_value}
              onChange={e =>
                setDestForm(p => ({
                  ...p,
                  sync_interval_value: parseInt(e.target.value) || 0,
                }))
              }
              required
            />
            <select
              className="app-input-text"
              value={destForm.sync_interval_unit}
              onChange={e =>
                setDestForm(p => ({
                  ...p,
                  sync_interval_unit: e.target.value as IntervalUnit,
                }))
              }
            >
              <option value="seconds">Seconds</option>
              <option value="minutes">Minutes</option>
              <option value="hours">Hours</option>
            </select>
          </div>
        </div>
        <div className="form-field">
          <div className="form-checkbox">
            <input
              type="checkbox"
              id="sync-all"
              checked={destForm.sync_all}
              onChange={e => setDestForm(p => ({ ...p, sync_all: e.target.checked }))}
            />
            <label htmlFor="sync-all">Sync all events (including past)</label>
          </div>
          <div className="form-checkbox">
            <input
              type="checkbox"
              id="keep-local"
              checked={destForm.keep_local}
              onChange={e => setDestForm(p => ({ ...p, keep_local: e.target.checked }))}
            />
            <label htmlFor="keep-local">Keep local CalDAV events not in ICS</label>
          </div>
        </div>
      </FormDialog>

      {/* ─── Delete confirmation dialog ─── */}
      <DeleteDialog
        prompt={deletePrompt}
        onCancel={() => setDeletePrompt(null)}
        onConfirm={handleDeleteConfirm}
      />
    </div>
  )
}
