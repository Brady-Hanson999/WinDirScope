import { useState } from "react";
import CyberBackground from "./CyberBackground";

export default function Landing({
  compact, path, setPath, drives,
  workers, setWorkers, depth, setDepth,
  onBrowse, onScan, scanning,
}) {
  const [showAdvanced, setShowAdvanced] = useState(false);

  const handleKeyDown = (e) => {
    if (e.key === "Enter" && !scanning) onScan();
  };

  const handleDriveChange = (e) => {
    if (e.target.value) setPath(e.target.value);
  };

  return (
    <div className={`landing${compact ? " compact" : ""}`}>
      {!compact && <CyberBackground />}
      {/* Brand */}
      <div className="landing-brand">
        <h1>WinDirScope</h1>
        {!compact && <p>Visualize every byte</p>}
      </div>

      {/* Search bar */}
      <div className="search-bar-wrap">
        <div className="search-bar">
          <select
            className="drive-select"
            value=""
            onChange={handleDriveChange}
            title="Select drive"
          >
            <option value="" disabled>Drive</option>
            {drives.map((d) => (
              <option key={d} value={d}>{d.replace("\\", "")}</option>
            ))}
          </select>

          <input
            className="path-input"
            type="text"
            value={path}
            onChange={(e) => setPath(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Enter a path to scan…"
            disabled={scanning}
            autoFocus={!compact}
          />

          <button
            className="browse-btn"
            onClick={onBrowse}
            disabled={scanning}
            title="Browse for folder"
          >
            📁
          </button>

          <button
            className="scan-btn"
            onClick={onScan}
            disabled={scanning || !path.trim()}
          >
            {scanning ? "Scanning…" : "Scan"}
          </button>
        </div>

        {/* Advanced options */}
        {!compact && (
          <div className="advanced-toggle">
            <button
              className="advanced-toggle-btn"
              onClick={() => setShowAdvanced(!showAdvanced)}
            >
              {showAdvanced ? "▾ Hide advanced" : "▸ Advanced options"}
            </button>

            {showAdvanced && (
              <div className="advanced-panel">
                <label>Workers</label>
                <input
                  type="range"
                  value={workers}
                  onChange={(e) => setWorkers(parseInt(e.target.value))}
                  min="1"
                  max="12"
                />
                <span className="slider-val">{workers}</span>

                <label>Max Depth</label>
                <input
                  type="number"
                  value={depth}
                  onChange={(e) => setDepth(e.target.value)}
                  placeholder="∞"
                  min="1"
                />
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
