import type { HealthInfo } from './api'
import {
  BrandImg,
  PROJECT_LOGO,
  distroIcon,
  distroLabel,
  osLabel,
} from './brandIcons'

type Props = {
  env: HealthInfo | null
  offline?: boolean
}

function guessOsFromBrowser(): string {
  const ua = navigator.userAgent.toLowerCase()
  if (ua.includes('win')) return 'windows'
  if (ua.includes('mac')) return 'macos'
  if (ua.includes('linux')) return 'linux'
  return 'unknown'
}

/** Project logo + distro/OS logo + environment chips. */
export default function EnvBrandBar({ env, offline }: Props) {
  const os = env?.os || guessOsFromBrowser()
  const distroId = env?.distro_id || os
  const label = distroLabel(env?.distro_name, distroId, os)
  const icon = distroIcon(distroId, os)
  const arch = env?.arch
  const host = env?.hostname
  const kernel = env?.kernel
  const release = env?.release
  const version = env?.version
  const wsl = Boolean(env?.wsl)

  return (
    <div className="env-brand-bar" aria-label="系统环境">
      <div className="env-brand-logos">
        <img
          src={PROJECT_LOGO}
          alt="Cocktail"
          className="project-logo"
          width={56}
          height={56}
        />
        <div className="env-brand-divider" aria-hidden />
        <div className="os-logo-wrap" title={label}>
          <BrandImg src={icon} alt={label} className="os-logo" height={28} />
        </div>
      </div>
      <div className="env-brand-info">
        <strong>Cocktail Manager</strong>
        <p className="env-distro-line">{label}</p>
        <div className="env-chips">
          <span className="env-chip">
            <BrandImg src={icon} alt="" height={12} />
            {distroId || osLabel(os)}
          </span>
          {wsl ? <span className="env-chip accent">WSL</span> : null}
          {arch ? <span className="env-chip">{arch}</span> : null}
          {kernel ? (
            <span className="env-chip" title="内核版本">
              <i className="fa fa-microchip" /> {kernel}
            </span>
          ) : null}
          {host ? (
            <span className="env-chip" title="主机名">
              <i className="fa fa-desktop" /> {host}
            </span>
          ) : null}
          {release ? <span className="env-chip">{release}</span> : null}
          {version ? (
            <span className="env-chip">v{version}</span>
          ) : offline ? (
            <span className="env-chip warn">控制面离线</span>
          ) : null}
          {env?.auth_required ? <span className="env-chip">Auth</span> : null}
          {!env?.os && !offline ? (
            <span className="env-chip" title="重启控制面后显示完整主机信息">
              浏览器推断
            </span>
          ) : null}
        </div>
      </div>
    </div>
  )
}
