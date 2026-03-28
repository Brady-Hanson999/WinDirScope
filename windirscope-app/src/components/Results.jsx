import { useState, useMemo } from "react";
import Treemap from "./Treemap";

export default function Results({
  result, rootChildren, skippedDirs, collectedErrors,
  formatBytes, onOpenResults, onNewScan, onShowErrors,
}) {
  // ── Sort state ──────────────────────────────────────────────────
  const [sortCol, setSortCol] = useState("bytes");
  const [sortAsc, setSortAsc] = useState(false);
  const [selectedIdx, setSelectedIdx] = useState(null);

  // Total bytes for % parent calculation
  const totalBytes = result.total_bytes || 1;

  // ── Sort logic ──────────────────────────────────────────────────
  const sortedChildren = useMemo(() => {
    const items = rootChildren.map((c, i) => ({ ...c, origIdx: i }));
    items.sort((a, b) => {
      let cmp = 0;
      if (sortCol === "name") {
        cmp = a.path.localeCompare(b.path);
      } else if (sortCol === "bytes") {
        cmp = a.bytes - b.bytes;
      } else if (sortCol === "pct") {
        cmp = a.bytes - b.bytes;
      }
      return sortAsc ? cmp : -cmp;
    });
    return items;
  }, [rootChildren, sortCol, sortAsc]);

  const handleSort = (col) => {
    if (sortCol === col) {
      setSortAsc(!sortAsc);
    } else {
      setSortCol(col);
      setSortAsc(col === "name");
    }
  };

  const sortArrow = (col) => {
    if (sortCol !== col) return "";
    return sortAsc ? " ▲" : " ▼";
  };

  // Size color coding
  const sizeColor = (bytes) => {
    const pct = (bytes / totalBytes) * 100;
    if (pct > 30) return "var(--red)";
    if (pct > 15) return "var(--peach)";
    if (pct > 5) return "var(--yellow)";
    return "var(--text)";
  };

  // Merge skipped dirs into total error/warning count for display
  const totalIssues = (result.error_count || 0) + (skippedDirs.count || 0);
  const allIssues = [
    ...collectedErrors,
    ...skippedDirs.paths.map((p) => `[Access Denied] ${p}`),
  ];

  return (
    <div className="results-section">
      {/* Header */}
      <div className="results-header">
        <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
          <h2>{result.cancelled ? "Scan Cancelled" : "Scan Complete"}</h2>
          <button className="new-scan-btn" onClick={onNewScan}>New Scan</button>
        </div>
        <button className="open-results-btn" onClick={onOpenResults}>
          Advanced View ↗
        </button>
      </div>

      {result.cancelled && (
        <div className="cancelled-badge">CANCELLED</div>
      )}

      {/* Summary cards */}
      <div className="summary-cards stagger">
        <div className="summary-card animate-fade-in-up">
          <div className="s-val" style={{ color: "var(--green)" }}>
            {formatBytes(result.total_bytes)}
          </div>
          <div className="s-label">Total Size</div>
        </div>
        <div className="summary-card animate-fade-in-up">
          <div className="s-val" style={{ color: "var(--blue)" }}>
            {result.total_files.toLocaleString()}
          </div>
          <div className="s-label">Files</div>
        </div>
        <div className="summary-card animate-fade-in-up">
          <div className="s-val" style={{ color: "var(--mauve)" }}>
            {result.total_dirs.toLocaleString()}
          </div>
          <div className="s-label">Directories</div>
        </div>
        <div className="summary-card animate-fade-in-up">
          <div className="s-val" style={{ color: "var(--yellow)" }}>
            {result.elapsed_secs.toFixed(2)}s
          </div>
          <div className="s-label">Elapsed</div>
        </div>
        {totalIssues > 0 ? (
          <button
            className="summary-card interactive animate-fade-in-up"
            onClick={onShowErrors}
          >
            <div className="s-val" style={{ color: "var(--red)" }}>
              {totalIssues.toLocaleString()}
            </div>
            <div className="s-label">Protected</div>
          </button>
        ) : (
          <div className="summary-card animate-fade-in-up">
            <div className="s-val" style={{ color: "var(--green)" }}>0</div>
            <div className="s-label">Protected</div>
          </div>
        )}
      </div>

      {/* ── Directory analysis table ─────────────────────────────── */}
      {sortedChildren.length > 0 && (
        <div className="dir-table-wrap animate-fade-in-up">
          <table className="dir-table">
            <thead>
              <tr>
                <th
                  className="dt-name sortable"
                  onClick={() => handleSort("name")}
                >
                  Name{sortArrow("name")}
                </th>
                <th
                  className="dt-size sortable"
                  onClick={() => handleSort("bytes")}
                >
                  Size{sortArrow("bytes")}
                </th>
                <th
                  className="dt-pct sortable"
                  onClick={() => handleSort("pct")}
                >
                  % Parent{sortArrow("pct")}
                </th>
              </tr>
            </thead>
            <tbody>
              {sortedChildren.map((c, i) => {
                const pct = ((c.bytes / totalBytes) * 100);
                const pctDisplay = pct < 0.01 ? "<0.01" : pct.toFixed(1);
                const isSelected = selectedIdx === i;
                return (
                  <tr
                    key={c.origIdx}
                    className={isSelected ? "selected" : ""}
                    onClick={() => setSelectedIdx(i)}
                    title={c.path}
                  >
                    <td className="dt-name">
                      <span className="dt-icon">📁</span>
                      <span className="dt-name-text">{c.path}</span>
                    </td>
                    <td className="dt-size" style={{ color: sizeColor(c.bytes) }}>
                      {formatBytes(c.bytes)}
                    </td>
                    <td className="dt-pct">
                      <div className="pct-bar-wrap">
                        <div
                          className="pct-bar-fill"
                          style={{ width: `${Math.min(pct, 100)}%` }}
                        />
                        <span className="pct-bar-label">{pctDisplay}%</span>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {/* Treemap */}
      <div className="result-panel animate-fade-in-up">
        <Treemap formatBytes={formatBytes} />
      </div>
    </div>
  );
}
