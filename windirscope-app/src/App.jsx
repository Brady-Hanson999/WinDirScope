import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/tauri";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/api/dialog";
import { WebviewWindow } from "@tauri-apps/api/window";
import "./App.css";

import Landing from "./components/Landing";
import Scanning from "./components/Scanning";
import Results from "./components/Results";
import ErrorModal from "./components/ErrorModal";

const MAX_ERRORS = 200;

function formatBytes(bytes) {
  if (bytes == null) return "0 B";
  const KB = 1024, MB = KB * 1024, GB = MB * 1024, TB = GB * 1024;
  if (bytes >= TB) return (bytes / TB).toFixed(2) + " TB";
  if (bytes >= GB) return (bytes / GB).toFixed(2) + " GB";
  if (bytes >= MB) return (bytes / MB).toFixed(2) + " MB";
  if (bytes >= KB) return (bytes / KB).toFixed(2) + " KB";
  return bytes + " B";
}

export { formatBytes };

export default function App() {
  // ── Phase ──────────────────────────────────────────────────────
  const [phase, setPhase] = useState("landing"); // "landing" | "scanning" | "results"

  // ── Input state ────────────────────────────────────────────────
  const [path, setPath] = useState("");
  const [drives, setDrives] = useState([]);
  const [workers, setWorkers] = useState(4);
  const [depth, setDepth] = useState("");

  // ── Admin ──────────────────────────────────────────────────────
  const [elevated, setElevated] = useState(false);
  const [elevating, setElevating] = useState(false);

  // ── Scan progress ──────────────────────────────────────────────
  const [progress, setProgress] = useState({
    dirs: 0, files: 0, bytes: 0, errors: 0, currentPath: "",
  });

  // ── Results ────────────────────────────────────────────────────
  const [result, setResult] = useState(null);
  const [rootChildren, setRootChildren] = useState([]);
  const [skippedDirs, setSkippedDirs] = useState({ count: 0, paths: [] });

  // ── Errors ─────────────────────────────────────────────────────
  const [errorModalOpen, setErrorModalOpen] = useState(false);
  const collectedErrors = useRef([]);

  // ── Refs ────────────────────────────────────────────────────────
  const resultsRef = useRef(null);

  // ── Initialize ─────────────────────────────────────────────────
  useEffect(() => {
    // Load drives
    invoke("list_drives")
      .then((d) => setDrives(d))
      .catch(() => {});

    // Check elevation
    invoke("is_elevated")
      .then((v) => setElevated(v))
      .catch(() => {});
  }, []);

  // ── Tauri event listeners ──────────────────────────────────────
  useEffect(() => {
    const unlisten = [];

    listen("scan_progress", (ev) => {
      const d = ev.payload;
      setProgress({
        dirs: d.dirs_scanned,
        files: d.total_files,
        bytes: d.total_bytes,
        errors: d.errors,
        currentPath: d.current_path || "",
      });
    }).then((u) => unlisten.push(u));

    listen("scan_error", (ev) => {
      const d = ev.payload;
      if (collectedErrors.current.length < MAX_ERRORS) {
        collectedErrors.current.push(`${d.path}: ${d.message}`);
      }
    }).then((u) => unlisten.push(u));

    listen("scan_finished", (ev) => {
      const d = ev.payload;
      setProgress({
        dirs: d.total_dirs,
        files: d.total_files,
        bytes: d.total_bytes,
        errors: d.error_count,
        currentPath: "",
      });
      setResult(d);

      // Skipped dirs
      if (d.skipped_dirs > 0) {
        setSkippedDirs({ count: d.skipped_dirs, paths: d.skipped_paths || [] });
      }

      // Load root children
      invoke("get_root_children")
        .then((children) => {
          const filtered = (children || []).filter((c) => c.bytes > 0);
          filtered.sort((a, b) => b.bytes - a.bytes);
          setRootChildren(filtered);
        })
        .catch(() => {});

      setPhase("results");

      // Smooth scroll to results after a tick
      setTimeout(() => {
        resultsRef.current?.scrollIntoView({ behavior: "smooth" });
      }, 100);
    }).then((u) => unlisten.push(u));

    return () => {
      unlisten.forEach((fn) => fn());
    };
  }, []);

  // ── Actions ────────────────────────────────────────────────────
  const handleBrowse = useCallback(async () => {
    const selected = await open({ directory: true, multiple: false });
    if (selected) setPath(selected);
  }, []);

  const handleScan = useCallback(async () => {
    const trimmed = path.trim();
    if (!trimmed) return;

    // Reset
    collectedErrors.current = [];
    setProgress({ dirs: 0, files: 0, bytes: 0, errors: 0, currentPath: "" });
    setResult(null);
    setRootChildren([]);
    setSkippedDirs({ count: 0, paths: [] });

    setPhase("scanning");

    try {
      await invoke("start_scan", {
        path: trimmed,
        workers: workers || null,
        depth: depth ? parseInt(depth) : null,
      });
    } catch (e) {
      console.error("[WinDirScope] scan error:", e);
      setPhase("landing");
    }
  }, [path, workers, depth]);

  const handleCancel = useCallback(async () => {
    try { await invoke("cancel_scan"); } catch (_) {}
  }, []);

  const handleElevate = useCallback(async () => {
    setElevating(true);
    try {
      await invoke("restart_elevated");
    } catch (e) {
      console.error("[WinDirScope] elevation failed:", e);
      setElevating(false);
    }
  }, []);

  const handleOpenResults = useCallback(() => {
    try {
      const w = new WebviewWindow("results-window", {
        url: "/results.html",
        title: "WinDirScope — Results",
        width: 1100,
        height: 750,
        resizable: true,
        visible: false, // Start hidden to prevent white flash
      });
      // Show once the webview is created and painted
      w.once('tauri://created', () => {
        setTimeout(() => w.show(), 120); // tiny delay lets React + CSS hydrate
      });
    } catch (e) {
      console.error("[WinDirScope] Failed to open results window:", e);
    }
  }, []);

  const handleNewScan = useCallback(() => {
    setPhase("landing");
    setResult(null);
    setRootChildren([]);
    setSkippedDirs({ count: 0, paths: [] });
    collectedErrors.current = [];
    setProgress({ dirs: 0, files: 0, bytes: 0, errors: 0, currentPath: "" });
  }, []);

  // ── Render ─────────────────────────────────────────────────────
  return (
    <div className="app">
      {/* Admin corner */}
      <div className="admin-corner">
        {elevated ? (
          <span className="admin-badge">✔ Elevated</span>
        ) : (
          <button
            className="admin-btn"
            onClick={handleElevate}
            disabled={elevating}
          >
            {elevating ? "Elevating…" : "🛡️ Admin"}
          </button>
        )}
      </div>

      {/* Phase 1: Landing (hidden during results) */}
      {phase !== "results" && (
        <Landing
          compact={phase !== "landing"}
          path={path}
          setPath={setPath}
          drives={drives}
          workers={workers}
          setWorkers={setWorkers}
          depth={depth}
          setDepth={setDepth}
          onBrowse={handleBrowse}
          onScan={handleScan}
          scanning={phase === "scanning"}
        />
      )}

      {/* Phase 2: Scanning */}
      {phase === "scanning" && (
        <Scanning
          progress={progress}
          onCancel={handleCancel}
          formatBytes={formatBytes}
        />
      )}

      {/* Phase 3: Results */}
      {(phase === "results" && result) && (
        <div ref={resultsRef}>
          <Results
            result={result}
            rootChildren={rootChildren}
            skippedDirs={skippedDirs}
            collectedErrors={collectedErrors.current}
            formatBytes={formatBytes}
            onOpenResults={handleOpenResults}
            onNewScan={handleNewScan}
            onShowErrors={() => setErrorModalOpen(true)}
          />
        </div>
      )}

      {/* Error Modal */}
      {errorModalOpen && (
        <ErrorModal
          errors={collectedErrors.current}
          skippedPaths={skippedDirs.paths}
          maxErrors={MAX_ERRORS}
          onClose={() => setErrorModalOpen(false)}
        />
      )}
    </div>
  );
}
