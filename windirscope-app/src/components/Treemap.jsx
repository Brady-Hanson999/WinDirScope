import { useState, useRef, useEffect, useCallback } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/tauri";

// ── Colour palette (same as original) ─────────────────────────────
const EXT_PALETTE = [
  "#f38ba8","#fab387","#f9e2af","#a6e3a1",
  "#94e2d5","#89dceb","#89b4fa","#cba6f7",
  "#f5c2e7","#b4befe","#74c7ec","#eba0ac",
  "#f2cdcd","#f5e0dc","#e6c99f","#bac2de",
];

const DIR_BASE_COLORS = [
  [30,34,52],[42,38,62],[38,50,58],[48,40,54],
  [40,48,44],[50,42,48],[44,44,56],[46,38,42],
];

function extHash(ext) {
  if (!ext) return 0;
  let h = 0;
  for (let i = 0; i < ext.length; i++) h = ((h << 5) - h + ext.charCodeAt(i)) | 0;
  return Math.abs(h);
}

function rectColor(r) {
  if (r.is_dir) {
    const [br,bg,bb] = DIR_BASE_COLORS[r.depth % DIR_BASE_COLORS.length];
    return `rgb(${br},${bg},${bb})`;
  }
  if (r.is_other) return "#45475a";
  if (!r.ext) return "#585b70";
  return EXT_PALETTE[extHash(r.ext) % EXT_PALETTE.length];
}

function escapeHtml(s) {
  const el = document.createElement("span");
  el.textContent = s;
  return el.innerHTML;
}

export default function Treemap({ formatBytes }) {
  const canvasRef = useRef(null);
  const [rects, setRects] = useState([]);
  const [hovered, setHovered] = useState(null);
  const [maxRects, setMaxRects] = useState(5000);
  const [depthLimit, setDepthLimit] = useState("");
  const [generating, setGenerating] = useState(false);
  const [status, setStatus] = useState("");
  const [breadcrumb, setBreadcrumb] = useState("");
  const [focusedPath, setFocusedPath] = useState(null);

  // Context menu state
  const [ctxMenu, setCtxMenu] = useState({ visible: false, x: 0, y: 0, rect: null, canDelete: false });

  // Delete confirmation dialog
  const [deleteConfirm, setDeleteConfirm] = useState({ visible: false, rect: null, deleting: false, error: null });
  
  // Toast notification
  const [toast, setToast] = useState({ visible: false, message: "", isError: false });

  // Tooltip state
  const [tooltip, setTooltip] = useState({ visible: false, x: 0, y: 0, html: "" });

  // ── Drawing ─────────────────────────────────────────────────────
  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    const dpr = window.devicePixelRatio || 1;
    const cssW = canvas.clientWidth;
    const cssH = canvas.clientHeight;
    canvas.width = cssW * dpr;
    canvas.height = cssH * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, cssW, cssH);

    for (let i = 0; i < rects.length; i++) {
      const r = rects[i];
      const px = r.x * cssW, py = r.y * cssH;
      const pw = r.w * cssW, ph = r.h * cssH;
      if (pw < 1 || ph < 1) continue;

      ctx.fillStyle = rectColor(r);
      ctx.fillRect(px, py, pw, ph);
      ctx.strokeStyle = "#1e1e2e";
      ctx.lineWidth = r.is_dir ? 1.2 : 0.4;
      ctx.strokeRect(px, py, pw, ph);

      // File labels
      if (!r.is_dir && pw > 30 && ph > 11) {
        const fs = Math.min(11, ph - 2);
        if (fs >= 7) {
          ctx.fillStyle = "#1e1e2e";
          ctx.font = `${fs}px "Segoe UI", sans-serif`;
          ctx.textBaseline = "middle";
          const mc = Math.floor(pw / (fs * 0.55));
          if (mc >= 2) {
            const txt = r.name.length > mc ? r.name.slice(0, mc - 1) + "…" : r.name;
            ctx.fillText(txt, px + 2, py + ph / 2);
          }
        }
      }

      // Dir header labels
      if (r.is_dir && pw > 40 && ph > 14) {
        const hPx = Math.min(ph * 0.08, cssH * 0.012);
        const fs = Math.min(11, Math.max(7, hPx - 1));
        if (fs >= 7 && hPx >= 8) {
          ctx.fillStyle = "#9399b2";
          ctx.font = `bold ${fs}px "Segoe UI", sans-serif`;
          ctx.textBaseline = "top";
          const mc = Math.floor(pw / (fs * 0.56));
          if (mc >= 2) {
            const txt = r.name.length > mc ? r.name.slice(0, mc - 1) + "…" : r.name;
            ctx.fillText(txt, px + 2, py + 1);
          }
        }
      }
    }

    // Highlight hovered
    if (hovered != null && hovered < rects.length) {
      const r = rects[hovered];
      const px = r.x * cssW, py = r.y * cssH;
      const pw = r.w * cssW, ph = r.h * cssH;
      ctx.strokeStyle = "#fff";
      ctx.lineWidth = 2;
      ctx.strokeRect(px, py, pw, ph);
      ctx.fillStyle = "rgba(255,255,255,0.06)";
      ctx.fillRect(px, py, pw, ph);
    }
  }, [rects, hovered]);

  useEffect(() => { draw(); }, [draw]);
  useEffect(() => {
    const onResize = () => { if (rects.length > 0) draw(); };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [rects, draw]);

  // ── Hit test ────────────────────────────────────────────────────
  const hitTest = (mx, my) => {
    const canvas = canvasRef.current;
    if (!canvas) return null;
    const cssW = canvas.clientWidth, cssH = canvas.clientHeight;
    const nx = mx / cssW, ny = my / cssH;
    for (let i = rects.length - 1; i >= 0; i--) {
      const r = rects[i];
      if (nx >= r.x && nx <= r.x + r.w && ny >= r.y && ny <= r.y + r.h) return i;
    }
    return null;
  };

  // ── Mouse handlers ──────────────────────────────────────────────
  const handleMouseMove = (e) => {
    const mx = e.nativeEvent.offsetX;
    const my = e.nativeEvent.offsetY;
    const idx = hitTest(mx, my);
    setHovered(idx);

    if (idx != null) {
      const r = rects[idx];
      const kind = r.is_dir ? "directory" : r.is_other ? "aggregated" : r.ext || "file";
      setTooltip({
        visible: true,
        x: e.clientX + 14,
        y: e.clientY + 14,
        html: `<strong>${escapeHtml(r.name)}</strong><br/>${escapeHtml(r.path)}<br/>${formatBytes(r.size)} · ${kind}`,
      });
    } else {
      setTooltip((t) => ({ ...t, visible: false }));
    }
  };

  const handleMouseLeave = () => {
    setHovered(null);
    setTooltip({ ...tooltip, visible: false });
  };

  const handleClick = (e) => {
    // Clicking should do nothing
    e.preventDefault();
  };

  const handleDoubleClick = (e) => {
    e.preventDefault();
    setCtxMenu((c) => ({ ...c, visible: false }));
    const mx = e.nativeEvent.offsetX;
    const my = e.nativeEvent.offsetY;
    const idx = hitTest(mx, my);
    if (idx == null) return;
    const r = rects[idx];
    
    // Focus the directory. If it's a file, focus its parent folder.
    let target = r.path;
    if (!r.is_dir) {
      const lastSlash = Math.max(r.path.lastIndexOf('\\'), r.path.lastIndexOf('/'));
      if (lastSlash > 0) target = r.path.substring(0, lastSlash);
    }
    
    // Trigger generation for this path
    handleGenerate(target);
  };

  const handleContextMenu = async (e) => {
    e.preventDefault();
    setCtxMenu((c) => ({ ...c, visible: false }));
    const mx = e.nativeEvent.offsetX;
    const my = e.nativeEvent.offsetY;
    const idx = hitTest(mx, my);
    if (idx == null) return;
    const r = rects[idx];
    setBreadcrumb(`${r.path} — ${formatBytes(r.size)}`);
    
    // Check if this path is safe to delete
    let canDelete = false;
    try {
      await invoke("check_delete_safety", { path: r.path });
      canDelete = true;
    } catch (_) {
      canDelete = false;
    }
    
    setCtxMenu({ visible: true, x: e.clientX, y: e.clientY, rect: r, canDelete });
  };

  // ── Context menu actions ────────────────────────────────────────
  const handleCtxOpen = async () => {
    setCtxMenu({ ...ctxMenu, visible: false });
    if (!ctxMenu.rect) return;
    try { await invoke("show_in_explorer", { path: ctxMenu.rect.path }); } catch (_) {}
  };

  const handleCtxCopy = () => {
    setCtxMenu({ ...ctxMenu, visible: false });
    if (!ctxMenu.rect) return;
    navigator.clipboard?.writeText(ctxMenu.rect.path).catch(() => {});
  };

  const handleCtxDelete = () => {
    setCtxMenu({ ...ctxMenu, visible: false });
    if (!ctxMenu.rect) return;
    setDeleteConfirm({ visible: true, rect: ctxMenu.rect, deleting: false, error: null });
  };

  const handleConfirmDelete = async () => {
    if (!deleteConfirm.rect) return;
    setDeleteConfirm(d => ({ ...d, deleting: true, error: null }));
    try {
      const msg = await invoke("delete_path", { path: deleteConfirm.rect.path });
      setDeleteConfirm({ visible: false, rect: null, deleting: false, error: null });
      showToast(msg, false);
      // Refresh treemap
      handleGenerate(focusedPath);
    } catch (err) {
      setDeleteConfirm(d => ({ ...d, deleting: false, error: typeof err === 'string' ? err : err.message || 'Deletion failed' }));
    }
  };

  const showToast = (message, isError) => {
    setToast({ visible: true, message, isError });
    setTimeout(() => setToast({ visible: false, message: "", isError: false }), 4000);
  };

  // Dismiss ctx menu on outside click
  useEffect(() => {
    const dismiss = (e) => {
      if (!e.target.closest(".ctx-menu") && !e.target.closest(".tm-canvas-wrap")) {
        setCtxMenu((c) => ({ ...c, visible: false }));
      }
    };
    const escDismiss = (e) => { if (e.key === "Escape") setCtxMenu((c) => ({ ...c, visible: false })); };
    document.addEventListener("click", dismiss);
    document.addEventListener("keydown", escDismiss);
    return () => { document.removeEventListener("click", dismiss); document.removeEventListener("keydown", escDismiss); };
  }, []);

  // ── Generate ────────────────────────────────────────────────────
  const handleGenerate = async (targetPath) => {
    // If event listener triggers this (targetPath is Event), or no arg, use current focus
    let p = focusedPath;
    if (targetPath === null || typeof targetPath === "string") {
      p = targetPath;
    }
    setGenerating(true);
    setBreadcrumb("");
    try {
      const data = await invoke("get_unified_treemap", {
        maxRects: maxRects,
        depthLimit: depthLimit ? parseInt(depthLimit) : null,
        rootPath: p || null,
      });
      setRects(data);
      setHovered(null);
      setFocusedPath(p || null);
      const fc = data.filter(r => !r.is_dir).length;
      const dc = data.filter(r => r.is_dir).length;
      setStatus(`${data.length} blocks (${dc} dirs, ${fc} files)`);
      setBreadcrumb(p ? `Focused: ${p}` : "Right click any block to show options");
    } catch (err) {
      console.error("[WinDirScope] treemap error:", err);
      if (p) {
        setFocusedPath(null); // Clear invalid focus
        setBreadcrumb(`Failed to focus ${p} (refreshing...)`);
      }
    } finally {
      setGenerating(false);
    }
  };

  return (
    <div className="treemap-section">
      <div
        className="result-panel-header"
        style={{ cursor: "default", padding: "12px 16px" }}
      >
        Treemap
      </div>
      <div style={{ padding: "0 16px 14px" }}>
        <div className="treemap-controls">
          <label>Max Blocks</label>
          <input
            type="range"
            value={maxRects}
            onChange={(e) => setMaxRects(parseInt(e.target.value))}
            min="1000" max="25000" step="500"
          />
          <span className="slider-val">{maxRects}</span>
          <label>Depth</label>
          <input
            type="number"
            value={depthLimit}
            onChange={(e) => setDepthLimit(e.target.value)}
            placeholder="∞"
            min="1"
          />
          <button
            className="treemap-generate-btn"
            onClick={() => handleGenerate(focusedPath)}
            disabled={generating}
          >
            {generating ? "Generating…" : "Generate"}
          </button>
          {focusedPath && (
            <button
              className="treemap-generate-btn"
              onClick={() => handleGenerate(null)}
              disabled={generating}
              style={{ background: "var(--surface1)", color: "var(--text)" }}
            >
              Reset Focus
            </button>
          )}
        </div>

        {status && <div className="tm-status">{status}</div>}

        <div className="tm-canvas-wrap">
          <canvas
            ref={canvasRef}
            onMouseMove={handleMouseMove}
            onMouseLeave={handleMouseLeave}
            onClick={handleClick}
            onDoubleClick={handleDoubleClick}
            onContextMenu={handleContextMenu}
          />
        </div>

        {breadcrumb && <div className="tm-breadcrumb">{breadcrumb}</div>}
      </div>

      {/* Tooltip via Portal */}
      {tooltip.visible && createPortal(
        <div
          className="tm-tooltip"
          style={{ left: tooltip.x, top: tooltip.y }}
          dangerouslySetInnerHTML={{ __html: tooltip.html }}
        />,
        document.body
      )}

      {/* Context Menu via Portal */}
      {ctxMenu.visible && createPortal(
        <div className="ctx-menu" style={{ left: ctxMenu.x, top: ctxMenu.y }}>
          <div className="ctx-item" onClick={handleCtxOpen}>
            {ctxMenu.rect?.is_dir ? "Open folder" : "Open file location"}
          </div>
          <div className="ctx-item" onClick={handleCtxCopy}>Copy path</div>
          <div className="ctx-divider" />
          {ctxMenu.canDelete ? (
            <div className="ctx-item ctx-item-danger" onClick={handleCtxDelete}>
              Delete
            </div>
          ) : (
            <div className="ctx-item ctx-item-disabled" title="This file/folder is protected and cannot be deleted">
              Protected
            </div>
          )}
        </div>,
        document.body
      )}

      {/* Delete Confirmation Dialog via Portal */}
      {deleteConfirm.visible && createPortal(
        <div className="delete-overlay" onClick={() => !deleteConfirm.deleting && setDeleteConfirm({ visible: false, rect: null, deleting: false, error: null })}>
          <div className="delete-dialog" onClick={e => e.stopPropagation()}>
            <h3 className="delete-dialog-title">Move to Recycle Bin?</h3>
            <p className="delete-dialog-path" title={deleteConfirm.rect?.path}>
              {deleteConfirm.rect?.name}
            </p>
            <div className="delete-dialog-details">
              <span>{deleteConfirm.rect?.is_dir ? 'Folder' : 'File'}</span>
              <span>·</span>
              <span>{formatBytes(deleteConfirm.rect?.size)}</span>
            </div>
            <p className="delete-dialog-fullpath">{deleteConfirm.rect?.path}</p>
            {deleteConfirm.error && (
              <div className="delete-dialog-error">{deleteConfirm.error}</div>
            )}
            <div className="delete-dialog-actions">
              <button 
                className="delete-dialog-cancel"
                onClick={() => setDeleteConfirm({ visible: false, rect: null, deleting: false, error: null })}
                disabled={deleteConfirm.deleting}
              >
                Cancel
              </button>
              <button 
                className="delete-dialog-confirm"
                onClick={handleConfirmDelete}
                disabled={deleteConfirm.deleting}
              >
                {deleteConfirm.deleting ? 'Deleting…' : 'Delete'}
              </button>
            </div>
          </div>
        </div>,
        document.body
      )}

      {/* Toast notification */}
      {toast.visible && createPortal(
        <div className={`delete-toast ${toast.isError ? 'delete-toast-error' : 'delete-toast-success'}`}>
          {toast.message}
        </div>,
        document.body
      )}
    </div>
  );
}
