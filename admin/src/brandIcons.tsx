/** Brand assets from Simple Icons + m3-Markdown-Badges. */

const SI = 'https://cdn.jsdelivr.net/npm/simple-icons@v13/icons'
const M3 = 'https://ziadoua.github.io/m3-Markdown-Badges/badges'

/** Monochrome SVG from Simple Icons (https://simpleicons.org/). */
export function simpleIcon(slug: string) {
  return `${SI}/${slug}.svg`
}

/** Material You badge from m3-Markdown-Badges. */
export function m3Badge(folder: string, file: string) {
  return `${M3}/${folder}/${file}.svg`
}

export const BRAND = {
  modrinth: simpleIcon('modrinth'),
  spigotmc: simpleIcon('spigotmc'),
  docker: simpleIcon('docker'),
  rust: simpleIcon('rust'),
  react: simpleIcon('react'),
  openjdk: simpleIcon('openjdk'),
  github: simpleIcon('github'),
  typescript: simpleIcon('typescript'),
  windows: 'https://cdn.jsdelivr.net/npm/simple-icons@v11/icons/windows.svg',
  linux: simpleIcon('linux'),
  apple: simpleIcon('apple'),
} as const

/** Project logo served from admin/public. */
export const PROJECT_LOGO = '/logo.png'

/** Distro / OS id → Simple Icons slug (https://simpleicons.org/). */
const DISTRO_SLUGS: Record<string, string> = {
  windows: 'windows',
  macos: 'apple',
  darwin: 'apple',
  linux: 'linux',
  ubuntu: 'ubuntu',
  debian: 'debian',
  fedora: 'fedora',
  archlinux: 'archlinux',
  arch: 'archlinux',
  centos: 'centos',
  rockylinux: 'rockylinux',
  rocky: 'rockylinux',
  almalinux: 'almalinux',
  alma: 'almalinux',
  opensuse: 'opensuse',
  suse: 'opensuse',
  gentoo: 'gentoo',
  alpinelinux: 'alpinelinux',
  alpine: 'alpinelinux',
  manjaro: 'manjaro',
  kalilinux: 'kalilinux',
  kali: 'kalilinux',
  popos: 'popos',
  pop: 'popos',
  linuxmint: 'linuxmint',
  mint: 'linuxmint',
  elementary: 'elementary',
  nixos: 'nixos',
  void: 'voidlinux',
  voidlinux: 'voidlinux',
  zorinos: 'zorinos',
  raspbian: 'raspberrypi',
  raspberrypi: 'raspberrypi',
  redhat: 'redhat',
  rhel: 'redhat',
  oraclelinux: 'oracle',
  amazon: 'amazon',
  amzn: 'amazon',
}

export function osIcon(os?: string) {
  switch ((os || '').toLowerCase()) {
    case 'windows':
      return BRAND.windows
    case 'macos':
    case 'darwin':
      return BRAND.apple
    case 'linux':
    default:
      return BRAND.linux
  }
}

export function osLabel(os?: string) {
  switch ((os || '').toLowerCase()) {
    case 'windows':
      return 'Windows'
    case 'macos':
    case 'darwin':
      return 'macOS'
    case 'linux':
      return 'Linux'
    default:
      return os || 'Unknown'
  }
}

/** Prefer distro-specific logo; fall back to generic OS logo. */
export function distroIcon(distroId?: string, os?: string) {
  const id = (distroId || '').toLowerCase()
  const slug = DISTRO_SLUGS[id]
  if (!slug) return osIcon(os)
  if (slug === 'windows') return BRAND.windows
  if (slug === 'apple') return BRAND.apple
  return simpleIcon(slug)
}

export function distroLabel(distroName?: string, distroId?: string, os?: string) {
  if (distroName && distroName.trim()) return distroName.trim()
  if (distroId && distroId !== os) return distroId
  return osLabel(os)
}

export const M3_BADGES = {
  docker: m3Badge('Docker', 'docker2'),
  rust: m3Badge('Rust', 'rust1'),
  react: m3Badge('React', 'react1'),
  typescript: m3Badge('TypeScript', 'typescript1'),
  github: m3Badge('Github', 'github1'),
  java: m3Badge('Java', 'java1'),
} as const

type BrandImgProps = {
  src: string
  alt: string
  className?: string
  height?: number
}

export function BrandImg({ src, alt, className, height = 18 }: BrandImgProps) {
  return (
    <img
      src={src}
      alt={alt}
      className={className ?? 'brand-img'}
      height={height}
      width={height}
      loading="lazy"
      decoding="async"
      referrerPolicy="no-referrer"
      onError={(e) => {
        // Unknown distro slug → fall back to generic Linux/Windows glyph
        const el = e.currentTarget
        if (el.dataset.fallback === '1') return
        el.dataset.fallback = '1'
        el.src = BRAND.linux
      }}
    />
  )
}
