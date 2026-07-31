/**
 * Origin that API paths are resolved against.
 *
 * Unlike the upstream template's helper this returns a bare origin rather than one
 * ending in `/api`, because callers here pass full `/api/...` paths.
 *
 * - `NEXT_PUBLIC_API_URL` set: use it verbatim (trailing slashes stripped), which lets
 *   the UI be pointed at a backend on another host.
 * - In the browser: empty, so requests stay relative and same-origin.
 * - Otherwise (server-side): the local Rust server.
 */
export function resolveApiBase(
  url: string | undefined,
  hasWindow: boolean,
  host?: string,
  port?: string
): string {
  if (url) return url.replace(/\/+$/, '')
  if (hasWindow) return ''
  return `http://${host || '127.0.0.1'}:${port || '6765'}`
}

const API_BASE = resolveApiBase(
  process.env.NEXT_PUBLIC_API_URL,
  typeof window !== 'undefined',
  process.env.SERVER_HOST,
  process.env.SERVER_PORT
)

export async function apiFetch<T = unknown>(
  url: string,
  options: RequestInit = {}
): Promise<{ data?: T; error?: string }> {
  try {
    const res = await fetch(`${API_BASE}${url}`, {
      ...options,
      headers: {
        ...(options.body !== undefined ? { 'Content-Type': 'application/json' } : {}),
        ...options.headers,
      },
    })
    const data = await res.json().catch(() => null)
    if (!res.ok) {
      return { error: data?.message || data?.error || `HTTP error ${res.status}` }
    }
    return { data }
  } catch (err) {
    return { error: err instanceof Error ? err.message : 'Network error' }
  }
}

export const api = {
  get: <T>(url: string) => apiFetch<T>(url, { method: 'GET' }),
  post: <T>(url: string, body?: unknown) =>
    apiFetch<T>(url, { method: 'POST', body: body ? JSON.stringify(body) : undefined }),
  put: <T>(url: string, body: unknown) =>
    apiFetch<T>(url, { method: 'PUT', body: JSON.stringify(body) }),
  delete: <T>(url: string) => apiFetch<T>(url, { method: 'DELETE' }),
}
