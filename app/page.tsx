'use client'

import React, { useState, useEffect, useCallback, ReactNode } from 'react'
import { api } from './api'

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
  public_ics: boolean
  public_ics_path: string | null
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

const emptySrcForm = {
  name: '',
  caldav_url: '',
  username: '',
  password: '',
  ics_path: '',
  sync_interval_hours: 1,
  sync_interval_minutes: 0,
  sync_interval_seconds: 0,
  public_ics: false,
  public_ics_path: '',
}

const emptyDestForm = {
  name: '',
  ics_url: '',
  caldav_url: '',
  calendar_name: '',
  username: '',
  password: '',
  sync_interval_hours: 1,
  sync_interval_minutes: 0,
  sync_interval_seconds: 0,
  sync_all: false,
  keep_local: false,
}

function toSecs(h: number, m: number, s: number): number {
  return h * 3600 + m * 60 + s
}

function fromSecs(secs: number): { hours: number; minutes: number; seconds: number } {
  const hours = Math.floor(secs / 3600)
  const minutes = Math.floor((secs % 3600) / 60)
  const seconds = secs % 60
  return { hours, minutes, seconds }
}

function generatePublicPath(name: string): string {
  const slug = (name || '')
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
  const random = Array.from(crypto.getRandomValues(new Uint8Array(6)), b =>
    b.toString(36).padStart(2, '0')
  )
    .join('')
    .slice(0, 8)
  return slug ? `${slug}/${random}` : random
}

function formatInterval(secs: number): string {
  if (secs === 0) return 'Manual only'
  const { hours, minutes, seconds } = fromSecs(secs)
  const parts: string[] = []
  if (hours > 0) parts.push(`${hours}h`)
  if (minutes > 0) parts.push(`${minutes}m`)
  if (seconds > 0 || parts.length === 0) parts.push(`${seconds}s`)
  return parts.join(' ')
}

function IntervalInput({
  hours,
  minutes,
  seconds,
  onChange,
}: {
  hours: number
  minutes: number
  seconds: number
  onChange: (field: string, value: number) => void
}) {
  return (
    <div className="form-field full-width">
      <label>Sync Interval (all zero = manual only)</label>
      <div className="interval-boxes">
        <div className="interval-box">
          <input
            className="app-input-text"
            type="number"
            min="0"
            value={hours}
            onChange={e => onChange('sync_interval_hours', parseInt(e.target.value) || 0)}
          />
          <span>Hours</span>
        </div>
        <div className="interval-box">
          <input
            className="app-input-text"
            type="number"
            min="0"
            max="59"
            value={minutes}
            onChange={e => onChange('sync_interval_minutes', parseInt(e.target.value) || 0)}
          />
          <span>Minutes</span>
        </div>
        <div className="interval-box">
          <input
            className="app-input-text"
            type="number"
            min="0"
            max="59"
            value={seconds}
            onChange={e => onChange('sync_interval_seconds', parseInt(e.target.value) || 0)}
          />
          <span>Seconds</span>
        </div>
      </div>
    </div>
  )
}

// --- Helpers ---

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false)

  return (
    <button
      type="button"
      className="app-btn app-btn-subtle"
      style={{ padding: '2px 8px', fontSize: 12, marginLeft: 4 }}
      onClick={e => {
        e.stopPropagation()
        navigator.clipboard.writeText(text)
        setCopied(true)
        setTimeout(() => setCopied(false), 2000)
      }}
    >
      {copied ? (
        <span style={{ color: '#0f0', fontWeight: 'bold' }}>Copied!</span>
      ) : (
        <i className="icons10-copy" />
      )}
    </button>
  )
}

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
          <span>+</span>
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
    const { data } = await api.get<{ sources: Source[] }>('/api/sources')
    if (data) setSources(data.sources)
  }, [])

  const fetchDestinations = useCallback(async () => {
    const { data } = await api.get<{ destinations: Destination[] }>('/api/destinations')
    if (data) setDestinations(data.destinations)
  }, [])

  const fetchHealth = useCallback(async () => {
    const { data } = await api.get<HealthStatus>('/api/health/detailed')
    setHealth(data ?? { status: 'unreachable' })
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

  async function apiSubmit(
    url: string,
    method: 'POST' | 'PUT',
    body: unknown,
    onSuccess: () => void
  ) {
    const { data, error } =
      method === 'POST'
        ? await api.post<{ message?: string }>(url, body)
        : await api.put<{ message?: string }>(url, body)

    if (error) {
      flash(error, 'error')
    } else {
      flash(data?.message || 'Success', 'success')
      onSuccess()
    }
  }

  async function apiSync(syncKeyPrefix: string, id: number, url: string, refresh: () => void) {
    setSyncing(p => ({ ...p, [`${syncKeyPrefix}-${id}`]: true }))
    try {
      const { data, error } = await api.post<{ message?: string }>(url)
      if (error) {
        flash(error, 'error')
      } else {
        flash(data?.message || 'Success', 'success')
        refresh()
      }
    } finally {
      setSyncing(p => ({ ...p, [`${syncKeyPrefix}-${id}`]: false }))
    }
  }

  async function apiDelete(url: string, refresh: () => void) {
    const { data, error } = await api.delete<{ message?: string }>(url)
    if (error) {
      flash(error, 'error')
    } else {
      flash(data?.message || 'Success', 'success')
      refresh()
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
    const { hours, minutes, seconds } = fromSecs(src.sync_interval_secs)
    setSrcForm({
      name: src.name,
      caldav_url: src.caldav_url,
      username: src.username,
      password: '',
      ics_path: src.ics_path,
      sync_interval_hours: hours,
      sync_interval_minutes: minutes,
      sync_interval_seconds: seconds,
      public_ics: src.public_ics,
      public_ics_path: src.public_ics_path || '',
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
    const { sync_interval_hours, sync_interval_minutes, sync_interval_seconds, ...rest } = srcForm
    const body = {
      ...rest,
      public_ics_path: rest.public_ics_path?.trim() || null,
      sync_interval_secs: toSecs(sync_interval_hours, sync_interval_minutes, sync_interval_seconds),
    }
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
    const { hours, minutes, seconds } = fromSecs(dest.sync_interval_secs)
    setDestForm({
      name: dest.name,
      ics_url: dest.ics_url,
      caldav_url: dest.caldav_url,
      calendar_name: dest.calendar_name,
      username: dest.username,
      password: '',
      sync_interval_hours: hours,
      sync_interval_minutes: minutes,
      sync_interval_seconds: seconds,
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
    const { sync_interval_hours, sync_interval_minutes, sync_interval_seconds, ...rest } = destForm
    const body = {
      ...rest,
      sync_interval_secs: toSecs(sync_interval_hours, sync_interval_minutes, sync_interval_seconds),
    }
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
    const origin = typeof window !== 'undefined' ? window.location.origin : ''
    const standardUrl = `${origin}/ics/${src.ics_path}`
    return (
      <>
        <div className="detail-row">
          <strong>ICS URL</strong>
          <span className="ics-url-row">
            <a href={`/ics/${src.ics_path}`} target="_blank" rel="noreferrer">
              {standardUrl}
            </a>
            <CopyButton text={standardUrl} />
          </span>
        </div>
        {src.public_ics && (
          <div className="detail-row">
            <strong>Public ICS URL</strong>
            <span className="ics-url-row">
              {src.public_ics_path ? (
                (() => {
                  const publicUrl = `${origin}/ics/public/${src.public_ics_path}`
                  return (
                    <>
                      <a
                        href={`/ics/public/${src.public_ics_path}`}
                        target="_blank"
                        rel="noreferrer"
                      >
                        {publicUrl}
                      </a>
                      <CopyButton text={publicUrl} />
                    </>
                  )
                })()
              ) : (
                <>
                  <span>Same as ICS URL (no auth required)</span>
                  <CopyButton text={standardUrl} />
                </>
              )}
            </span>
          </div>
        )}
      </>
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
        <IntervalInput
          hours={srcForm.sync_interval_hours}
          minutes={srcForm.sync_interval_minutes}
          seconds={srcForm.sync_interval_seconds}
          onChange={(field, value) => setSrcForm(p => ({ ...p, [field]: value }))}
        />
        <div className="form-field full-width">
          <div className="form-checkbox">
            <input
              type="checkbox"
              id="public-ics"
              checked={srcForm.public_ics}
              onChange={e => {
                const checked = e.target.checked
                setSrcForm(p => ({
                  ...p,
                  public_ics: checked,
                  public_ics_path: checked ? generatePublicPath(p.name || p.username) : '',
                }))
              }}
            />
            <label htmlFor="public-ics">
              Make ICS URL public (for services like Google Calendar that don't support HTTP Basic
              Auth)
            </label>
          </div>
          {srcForm.public_ics && (
            <div style={{ marginTop: 8 }}>
              <label style={{ fontSize: 13, opacity: 0.8, display: 'block', marginBottom: 6 }}>
                Custom public path (leave empty to make standard URL public)
              </label>
              <input
                className="app-input-text"
                type="text"
                value={srcForm.public_ics_path}
                onChange={e => setSrcForm(p => ({ ...p, public_ics_path: e.target.value }))}
                placeholder="e.g. my-calendar/abc123"
              />
            </div>
          )}
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
        <IntervalInput
          hours={destForm.sync_interval_hours}
          minutes={destForm.sync_interval_minutes}
          seconds={destForm.sync_interval_seconds}
          onChange={(field, value) => setDestForm(p => ({ ...p, [field]: value }))}
        />
        <div className="form-field full-width">
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
