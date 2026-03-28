export default function Scanning({ progress, onCancel, formatBytes }) {
  return (
    <div className="scanning-section">
      {/* Status row */}
      <div className="scan-status-row">
        <div className="scan-pulse" />
        <span className="scan-status-label">Scanning…</span>
        <button className="cancel-btn" onClick={onCancel}>Cancel</button>
      </div>

      {/* Stat cards */}
      <div className="stat-cards stagger">
        <div className="stat-card files animate-fade-in-up">
          <div className="stat-value">{progress.files.toLocaleString()}</div>
          <div className="stat-label">Files</div>
        </div>
        <div className="stat-card dirs animate-fade-in-up">
          <div className="stat-value">{progress.dirs.toLocaleString()}</div>
          <div className="stat-label">Directories</div>
        </div>
        <div className="stat-card size animate-fade-in-up">
          <div className="stat-value">{formatBytes(progress.bytes)}</div>
          <div className="stat-label">Total Size</div>
        </div>
        <div className="stat-card errors animate-fade-in-up">
          <div className="stat-value">{progress.errors.toLocaleString()}</div>
          <div className="stat-label">Errors</div>
        </div>
      </div>

      {/* Current path */}
      <div className="current-path">{progress.currentPath}</div>
    </div>
  );
}
