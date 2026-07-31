import { describe, expect, test } from 'bun:test'
import { resolveApiBase } from '../api'

describe('resolveApiBase', () => {
  test('NEXT_PUBLIC_API_URL set, trailing slashes stripped', () => {
    expect(resolveApiBase('https://x.com//', false)).toBe('https://x.com')
  })

  test('url wins over window', () => {
    expect(resolveApiBase('https://x.com', true)).toBe('https://x.com')
  })

  test('unset with window -> relative, same-origin', () => {
    expect(resolveApiBase(undefined, true)).toBe('')
  })

  test('unset without window -> host/port', () => {
    expect(resolveApiBase(undefined, false, 'srv', '9000')).toBe('http://srv:9000')
  })

  test('unset without window, no host/port -> defaults', () => {
    expect(resolveApiBase(undefined, false)).toBe('http://127.0.0.1:6765')
    expect(resolveApiBase(undefined, false, '', '')).toBe('http://127.0.0.1:6765')
  })

  test('composes with the /api paths callers pass', () => {
    expect(`${resolveApiBase(undefined, true)}/api/sources`).toBe('/api/sources')
    expect(`${resolveApiBase('https://x.com/', false)}/api/sources`).toBe(
      'https://x.com/api/sources'
    )
  })
})
