import { useState, useRef, useEffect, useCallback } from "react";
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

  // Context menu state
  const [ctxMenu, setCtxMenu] = useState({ visible: false, x: 0, y: 0, rect: null });

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
    const canvas = canvasRef.current;
    const rect = canvas.getBoundingClientRect();
    const mx = e.clientX - rect.left, my = e.clientY - rect.top;
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
      setTooltip({ ...tooltip, visible: false });
    }
  };

  const handleMouseLeave = () => {
    setHovered(null);
    setTooltip({ ...tooltip, visible: false });
  };

  const handleClick = (e) => {
    setCtxMenu({ ...ctxMenu, visible: false });
    const canvas = canvasRef.current;
    const rect = canvas.getBoundingClientRect();
    const idx = hitTest(e.clientX - rect.left, e.clientY - rect.top);
    if (idx == null) return;
    const r = rects[idx];
    setBreadcrumb(`${r.path} — ${formatBytes(r.size)}`);
    setCtxMenu({ visible: true, x: e.clientX, y: e.clientY, rect: r });
  };

  const handleContextMenu = (e) => {
    e.preventDefault();
    handleClick(e);
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
  const handleGenerate = async () => {
    setGenerating(true);
    setBreadcrumb("");
    try {
      const data = await invoke("get_unified_treemap", {
        maxRects: maxRects,
        depthLimit: depthLimit ? parseInt(depthLimit) : null,
      });
      setRects(data);
      setHovered(null);
      const fc = data.filter(r => !r.is_dir).length;
      const dc = data.filter(r => r.is_dir).length;
      setStatus(`${data.length} blocks (${dc} dirs, ${fc} files)`);
      setBreadcrumb("Click any block to open its location");
    } catch (err) {
      console.error("[WinDirScope] treemap error:", err);
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
            onClick={handleGenerate}
            disabled={generating}
          >
            {generating ? "Generating…" : "Generate"}
          </button>
        </div>

        {status && <div className="tm-status">{status}</div>}

        <div className="tm-canvas-wrap">
          <canvas
            ref={canvasRef}
            onMouseMove={handleMouseMove}
            onMouseLeave={handleMouseLeave}
            onClick={handleClick}
            onContextMenu={handleContextMenu}
          />
        </div>

        {breadcrumb && <div className="tm-breadcrumb">{breadcrumb}</div>}
      </div>

      {/* Tooltip */}
      {tooltip.visible && (
        <div
          className="tm-tooltip"
          style={{ left: tooltip.x, top: tooltip.y }}
          dangerouslySetInnerHTML={{ __html: tooltip.html }}
        />
      )}

      {/* Context Menu */}
      {ctxMenu.visible && (
        <div className="ctx-menu" style={{ left: ctxMenu.x, top: ctxMenu.y }}>
          <div className="ctx-item" onClick={handleCtxOpen}>
            {ctxMenu.rect?.is_dir ? "Open folder" : "Open file location"}
          </div>
          <div className="ctx-item" onClick={handleCtxCopy}>Copy path</div>
        </div>
      )}
    </div>
  );
}
