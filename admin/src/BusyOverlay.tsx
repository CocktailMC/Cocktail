import { formatBytes, type DownloadProgress } from './api'

type Props = {
  active: boolean
  label?: string
  /** Non-blocking status strip (e.g. starting/stopping) */
  statusHint?: string | null
  progress?: DownloadProgress | null
}

const PHASE_LABEL: Record<string, string> = {
  download: '下载中',
  extract: '解压安装中',
  install: '安装中',
  done: '完成',
}

/** Top bar + optional blocking wait panel, with determinate download progress. */
export default function BusyOverlay({ active, label, statusHint, progress }: Props) {
  const pct =
    progress?.pct != null && Number.isFinite(progress.pct)
      ? Math.max(0, Math.min(100, progress.pct))
      : null
  const determinate = pct != null && progress?.phase === 'download'
  const showBar = active || Boolean(statusHint) || Boolean(progress)
  const title = progress?.label || label || '处理中…'
  const phaseText = progress
    ? PHASE_LABEL[progress.phase] || progress.phase
    : null
  const bytesText =
    progress && progress.phase === 'download'
      ? progress.total
        ? `${formatBytes(progress.received)} / ${formatBytes(progress.total)}`
        : formatBytes(progress.received)
      : null

  if (!showBar && !active) return null

  return (
    <>
      {showBar && (
        <div
          className={`load-bar ${active || progress ? 'active' : 'hint'}`}
          role="progressbar"
          aria-busy={active}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={determinate ? Math.round(pct) : undefined}
          aria-label={title}
        >
          {determinate ? (
            <div className="load-bar-fill" style={{ width: `${pct}%` }} />
          ) : (
            <div className="load-bar-indeterminate" />
          )}
        </div>
      )}
      {active && (
        <div className="busy-overlay" role="alertdialog" aria-modal="true">
          <div className="busy-card">
            <div className="busy-spinner" aria-hidden />
            <p className="busy-title">{title}</p>
            <p className="busy-sub">
              {phaseText
                ? `${phaseText}${bytesText ? ` · ${bytesText}` : ''}${
                    determinate ? ` · ${Math.round(pct)}%` : ''
                  }`
                : '请稍候，完成后会自动刷新'}
            </p>
            <div
              className={`busy-progress ${determinate ? 'determinate' : 'pulse'}`}
            >
              <span style={determinate ? { width: `${pct}%` } : undefined} />
            </div>
          </div>
        </div>
      )}
      {!active && statusHint && (
        <div className="status-toast">{statusHint}</div>
      )}
    </>
  )
}
