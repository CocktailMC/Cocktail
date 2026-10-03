export type HomeTab =
  | 'overview'
  | 'settings'
  | 'audit'
  | 'nodes'
  | 'extensions'
  | 'network'
  | 'events'
  | 'users'

export type HashLoc =
  | { kind: 'home'; tab: HomeTab }
  | { kind: 'plugin'; id: string }
  | { kind: 'instance'; id: string; tab: string }
  | { kind: 'create' }
  | { kind: 'eula'; id: string }

const HOME_TABS: HomeTab[] = [
  'overview',
  'settings',
  'audit',
  'nodes',
  'extensions',
  'network',
  'events',
  'users',
]

export function parseHash(): HashLoc {
  const raw = window.location.hash.replace(/^#/, '').replace(/^\//, '')
  const parts = raw.split('/').filter(Boolean)
  if (parts[0] === 'create') return { kind: 'create' }
  if (parts[0] === 'eula' && parts[1]) return { kind: 'eula', id: parts[1] }
  if (parts[0] === 'instances' && parts[1]) {
    return { kind: 'instance', id: parts[1], tab: parts[2] || 'console' }
  }
  if (parts[0] === 'plugins' && parts[1]) {
    return { kind: 'plugin', id: parts[1] }
  }
  if (parts[0] && HOME_TABS.includes(parts[0] as HomeTab)) {
    return { kind: 'home', tab: parts[0] as HomeTab }
  }
  return { kind: 'home', tab: 'overview' }
}

export function writeHash(loc: HashLoc) {
  const next =
    loc.kind === 'home'
      ? loc.tab === 'overview'
        ? '#/'
        : `#/${loc.tab}`
      : loc.kind === 'plugin'
        ? `#/plugins/${loc.id}`
        : loc.kind === 'instance'
          ? `#/instances/${loc.id}/${loc.tab}`
          : loc.kind === 'create'
            ? '#/create'
            : `#/eula/${loc.id}`
  if (window.location.hash !== next) {
    window.location.hash = next
  }
}
