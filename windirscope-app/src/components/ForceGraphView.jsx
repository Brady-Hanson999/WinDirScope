import { useState, useRef, useEffect, useCallback } from 'react';
import ForceGraph3D from 'react-force-graph-3d';
import { invoke } from '@tauri-apps/api/tauri';
import { createPortal } from "react-dom";
import "../App.css";
import HoloExplorer from './HoloExplorer';

// We removed Catppuccin mapped colours to obey user's direct request 
// regarding size-based blue-to-dark-red gradient logic.
function getNodeColorBySize(sizeBytes) {
  if (!sizeBytes) sizeBytes = 0;

  // We use a modified log scaling factor so files/folders are 
  // scaled between 0 and 1 gracefully relative to typical disk sizes (1MB to 10GB reference)
  // Shift scale slightly so normal directories get mixed colors
  let ratio = Math.pow(sizeBytes / (10 * 1024 * 1024 * 1024), 0.35); 
  if (ratio > 1) ratio = 1;
  if (ratio < 0) ratio = 0;
  
  // Light blue: r=137, g=220, b=235 (Catppuccin Sky)
  // Dark red: r=138, g=20, b=30 
  const r = Math.round(137 + (138 - 137) * ratio);
  const g = Math.round(220 + (20 - 220) * ratio);
  const b = Math.round(235 + (30 - 235) * ratio);
  
  return `rgb(${r},${g},${b})`;
}

function formatBytes(bytes) {
  if (bytes == null) return "0 B";
  const KB = 1024, MB = KB * 1024, GB = MB * 1024, TB = GB * 1024;
  if (bytes >= TB) return (bytes / TB).toFixed(2) + " TB";
  if (bytes >= GB) return (bytes / GB).toFixed(2) + " GB";
  if (bytes >= MB) return (bytes / MB).toFixed(2) + " MB";
  if (bytes >= KB) return (bytes / KB).toFixed(2) + " KB";
  return bytes + " B";
}

function DuplicatesModal({ onClose, rootPath, onAddToCart }) {
  const [groups, setGroups] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(null);

  useEffect(() => {
    invoke("find_duplicates", { path: rootPath || "C:\\" })
      .then(res => setGroups(res))
      .catch(err => setError(String(err)))
      .finally(() => setLoading(false));
  }, [rootPath]);

  return createPortal(
    <div className="modal-overlay" onClick={onClose} style={{ zIndex: 600 }}>
      <div className="modal" onClick={e => e.stopPropagation()} style={{ width: '700px', maxWidth: '95vw', background: 'var(--base)', border: '1px solid var(--surface2)' }}>
        <div className="modal-header">
          <h2>Duplicate Files {rootPath ? `in ${rootPath}` : 'in Root'}</h2>
          <button className="modal-close-btn" onClick={onClose}>×</button>
        </div>
        <div className="modal-body" style={{ padding: '0', display: 'flex', flexDirection: 'column', gap: '8px', maxHeight: '60vh', overflowY: 'auto' }}>
          {loading && <div style={{ padding: '24px', textAlign: 'center', color: 'var(--subtext0)' }}>Scanning for exact binary duplicates (this may take a minute)...</div>}
          {error && <div style={{ padding: '24px', color: 'var(--red)' }}>{error}</div>}
          {(!loading && !error && groups.length === 0) && (
            <div style={{ padding: '24px', textAlign: 'center', color: 'var(--green)' }}>No duplicates found! Your files are squeaky clean.</div>
          )}
          {groups.map((g, i) => (
            <div key={i} style={{ borderBottom: '1px solid var(--surface1)', padding: '12px 16px' }}>
              <div style={{ fontWeight: 'bold', color: 'var(--yellow)', marginBottom: '8px' }}>
                Group {i+1} — {formatBytes(g.size)} each
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                {g.files.map((f, j) => (
                  <div key={j} style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', background: 'var(--surface0)', padding: '6px 10px', borderRadius: '6px' }}>
                    <span style={{ fontSize: '0.8rem', color: 'var(--subtext0)', wordBreak: 'break-all', paddingRight: '12px' }}>{f.path}</span>
                    <button 
                      onClick={() => onAddToCart({ path: f.path, name: f.name, size: f.size, is_dir: false })}
                      style={{ background: 'var(--surface1)', border: '1px solid var(--surface2)', color: 'var(--text)', padding: '4px 8px', borderRadius: '4px', cursor: 'pointer', fontSize: '0.75rem', whiteSpace: 'nowrap' }}
                    >
                      Queue
                    </button>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>,
    document.body
  );
}

export default function ForceGraphView() {
  const fgRef = useRef();
  const [graphData, setGraphData] = useState({ nodes: [], links: [] });
  const [generating, setGenerating] = useState(false);
  const [maxNodes, setMaxNodes] = useState(2000);
  const [focusedPath, setFocusedPath] = useState(null);
  const [selectedNode, setSelectedNode] = useState(null);
  
  const [hoverNode, setHoverNode] = useState(null);
  const [highlightNodes, setHighlightNodes] = useState(new Set());
  const [highlightLinks, setHighlightLinks] = useState(new Set());
  
  const [status, setStatus] = useState("");
  const [tooltip, setTooltip] = useState({ visible: false, x: 0, y: 0, html: "" });

  const [cart, setCart] = useState([]);
  const [toast, setToast] = useState({ visible: false, message: "", isError: false });
  const [deletingCart, setDeletingCart] = useState(false);
  
  const [showDuplicates, setShowDuplicates] = useState(false);

  const showToast = (message, isError = false) => {
    setToast({ visible: true, message, isError });
    setTimeout(() => setToast({ visible: false, message: "", isError: false }), 4000);
  };

  const loadGraph = useCallback(async (targetPath = null) => {
    setGenerating(true);
    setStatus("Generating 3D Graph...");
    setHighlightNodes(new Set());
    setHighlightLinks(new Set());
    setSelectedNode(null);
    try {
      const data = await invoke("get_graph_data", {
        maxNodes: 250,
        depthLimit: 1, // Layer by layer mode
        rootPath: targetPath
      });
      
      // Pin the parent folder to the absolute center of the physics engine
      if (data && data.nodes) {
        data.nodes.forEach((n, idx) => {
          if (n.depth === 0) {
            n.fx = 0;
            n.fy = 0;
            n.fz = 0;
          } else {
            // Nudge children explicitly on generation so single-children clusters have a vector 
            // for the repulsive forces to push them away gracefully instead of NaN overlapping the parent.
            n.x = Math.cos(idx) * 50 + (Math.random() - 0.5) * 10; 
            n.y = Math.sin(idx) * 50 + (Math.random() - 0.5) * 10;
            n.z = (Math.random() - 0.5) * 10;
          }
        });
      }

      setGraphData(data);
      setFocusedPath(targetPath);
      setStatus(`Showing ${data.nodes.length} nodes`);
    } catch (err) {
      console.error("[WinDirScope] Graph Error", err);
      if (targetPath) {
        setFocusedPath(null);
        setStatus("Failed to load subset.");
      }
    } finally {
      setGenerating(false);
    }
  }, []);

  useEffect(() => {
    loadGraph(focusedPath);
  }, []);

  const handlePointerMove = useCallback((e) => {
    if (hoverNode) {
      setTooltip(prev => ({ ...prev, x: e.clientX, y: e.clientY }));
    }
  }, [hoverNode]);

  useEffect(() => {
    window.addEventListener('mousemove', handlePointerMove);
    return () => window.removeEventListener('mousemove', handlePointerMove);
  }, [handlePointerMove]);

  // Configure WebGL Camera Controls and Physics Engine
  useEffect(() => {
    if (fgRef.current) {
      const controls = fgRef.current.controls();
      // Lock rotation, keeping it strictly 2D facing
      controls.enableRotate = false;
      // Disable panning completely
      controls.enablePan = false; 
      
      controls.minPolarAngle = 0;
      controls.maxPolarAngle = 0;
      controls.minAzimuthAngle = 0;
      controls.maxAzimuthAngle = 0;
      
      // Remove panning mouse bindings
      controls.mouseButtons = {
        MIDDLE: 1 // Zoom only
      };

      // Physics: Make the dots more spread out!
      fgRef.current.d3Force('charge').strength(-300); // Stronger repulsion spreading nodes (default is usually ~ -30)
      fgRef.current.d3Force('link').distance(100);    // Pushes links further apart natively
    }
  }, [graphData]); // Re-apply if graph remounts

  // Recursively collect subtree
  const getSubtree = useCallback((node) => {
    const nodeIds = new Set([node.id]);
    const linkIds = new Set();
    
    // Naive BFS through graphData.links using react-force-graph internal structure
    // react-force-graph replaces link.source/target strings with object references!
    let changed = true;
    while(changed) {
      changed = false;
      for (const link of graphData.links) {
        const sourceId = link.source.id || link.source;
        const targetId = link.target.id || link.target;
        
        if (nodeIds.has(sourceId) && !nodeIds.has(targetId)) {
          nodeIds.add(targetId);
          linkIds.add(link);
          changed = true;
        }
      }
    }
    return { nodes: nodeIds, links: linkIds };
  }, [graphData]);

  const handleGoUp = useCallback(() => {
    if (!focusedPath) return; 
    let lastSlash = focusedPath.lastIndexOf('\\');
    if (lastSlash < 0) lastSlash = focusedPath.lastIndexOf('/');
    if (lastSlash <= 2) {
      loadGraph(null); 
    } else {
      loadGraph(focusedPath.substring(0, lastSlash));
    }
  }, [focusedPath, loadGraph]);

  const handleNodeClick = useCallback(node => {
    // Zoom/camera shifting removed completely natively locking it
    if (node.is_dir && node.depth !== 0) {
      loadGraph(node.path);
      return; 
    }

    setSelectedNode(node);

    if (node.is_dir) {
      const { nodes, links } = getSubtree(node);
      setHighlightNodes(nodes);
      setHighlightLinks(links);
    } else {
      setHighlightNodes(new Set([node.id]));
      setHighlightLinks(new Set());
    }
  }, [getSubtree, loadGraph]);

  const handleNodeRightClick = async (node) => {
    if (cart.find(n => n.path === node.path)) {
      setCart(c => c.filter(n => n.path !== node.path));
      showToast("Removed from deletion queue");
      return;
    }
    try {
      await invoke("check_delete_safety", { path: node.path });
      setCart(c => [...c, node]);
      showToast(`Added ${node.name} to deletion queue`);
    } catch (err) {
      showToast(typeof err === 'string' ? err : "This file is protected", true);
    }
  };

  return (
    <div className="graph-view-container" style={{ width: '100%', height: '100%', background: '#11111b', overflow: 'hidden', position: 'absolute', top: 0, left: 0 }}>
      {/* HUD Controls */}
      <div className="treemap-hud" style={{ position: 'absolute', top: '16px', left: '16px', zIndex: 10, display: 'flex', gap: '8px', background: 'rgba(30, 30, 46, 0.8)', padding: '12px 20px', borderRadius: '12px', backdropFilter: 'blur(10px)', border: '1px solid rgba(255,255,255,0.05)' }}>
        <h3 style={{ margin: 0, color: '#cdd6f4' }}>WinDirScope 3D</h3>
        
        <button onClick={() => loadGraph(focusedPath)} disabled={generating} className="treemap-generate-btn">
          {generating ? "Loading..." : "Reload"}
        </button>

        {focusedPath && (
          <button onClick={() => loadGraph(null)} className="treemap-generate-btn" style={{ background: 'var(--surface1)'}}>
            Top Root
          </button>
        )}

        {highlightNodes.size > 0 && (
          <button onClick={() => { setHighlightNodes(new Set()); setHighlightLinks(new Set()); setSelectedNode(null); }} className="treemap-generate-btn" style={{ background: 'var(--surface1)'}}>
            Clear Highlight
          </button>
        )}

        <button onClick={() => setShowDuplicates(true)} className="treemap-generate-btn" style={{ background: 'var(--surface1)'}}>
          Find Duplicates
        </button>

        {selectedNode && (
          <button 
            onClick={() => handleNodeRightClick(selectedNode)}
            className="treemap-generate-btn" 
            style={{ 
              background: cart.find(n => n.path === selectedNode.path) ? 'var(--surface2)' : 'var(--red)', 
              color: cart.find(n => n.path === selectedNode.path) ? 'var(--text)' : 'var(--crust)',
              fontWeight: 'bold', border: 'none'
            }}
          >
            {cart.find(n => n.path === selectedNode.path) ? `Remove from Queue` : `Queue for Deletion`}
          </button>
        )}

        <span style={{ fontSize: '0.9rem', color: 'var(--subtext0)', alignSelf: 'center', marginLeft: '8px' }}>{status}</span>
        <span style={{ fontSize: '0.8rem', color: 'var(--overlay0)', marginLeft: '8px', alignSelf: 'center', borderLeft: '1px solid var(--surface2)', paddingLeft: '12px' }}>Right-click or select nodes to queue for deletion</span>
      </div>

      <ForceGraph3D
        ref={fgRef}
        graphData={graphData}
        numDimensions={2}
        nodeLabel={() => ''} // custom tooltip overlay
        nodeColor={node => {
          if (cart.find(n => n.path === node.path)) return '#f38ba8'; // Highlight cart items in red
          if (highlightNodes.size > 0 && !highlightNodes.has(node.id)) {
            return `rgba(186, 194, 222, 0.05)`; // super dimmed
          }
          return getNodeColorBySize(node.size); 
        }}
        nodeVal={node => {
          // Increase size divergence drastically to make sizes noticeable
          const sizeMB = (node.size || 1) / (1024 * 1024);
          let val = Math.max(0.5, Math.pow(sizeMB, 0.45));
          if (node.depth === 0) val = val * 1.5; // Bump the central root slightly larger!
          return val;
        }}
        nodeOpacity={1.0}
        linkWidth={link => highlightLinks.has(link) ? 2 : 0.5}
        linkColor={link => highlightLinks.has(link) ? 'rgba(255,255,255,0.8)' : 'rgba(255,255,255,0.05)'}
        enableNodeDrag={false}
        backgroundColor="#11111b"
        onNodeHover={node => {
          setHoverNode(node);
          if (node) {
            setTooltip(t => ({ 
              visible: true, 
              x: t.x || window.innerWidth/2, 
              y: t.y || window.innerHeight/2, 
              html: `<strong>${node.name}</strong><br/>${formatBytes(node.size)}<br/><span style="color:var(--subtext0)">${node.path}</span>` 
            }));
          } else {
            setTooltip(t => ({ ...t, visible: false }));
          }
        }}
        onNodeClick={handleNodeClick}
        onNodeRightClick={handleNodeRightClick}
        d3Force={(d3, forceEngine) => {
          if (fgRef.current && forceEngine === "d3") {
            // Apply physics on map building
            fgRef.current.d3Force('charge').strength(-300); // Spreads graph
            fgRef.current.d3Force('link').distance(150);    // Pushes links further
            
            // Apply anti-overlap bounding force so different sized nodes don't collide
            fgRef.current.d3Force('collide', d3.forceCollide(node => {
               const sizeMB = (node.size || 1) / (1024 * 1024);
               let val = Math.max(0.5, Math.pow(sizeMB, 0.45));
               if (node.depth === 0) val = val * 1.5;
               
               // react-force-graph spheres have physical radius cbrt(val).
               // Multiply by strict scalar + padding to ensure bounds are respected literally
               return Math.cbrt(val * (3/4 * Math.PI)) * 10 + 4; 
            }));
          }
        }}
      />

      <HoloExplorer 
        graphData={graphData} 
        focusedPath={focusedPath}
        onGoUp={handleGoUp}
        onNodeClick={handleNodeClick} 
        onNodeHover={node => {
          setHoverNode(node);
          if (!node) setTooltip(t => ({...t, visible: false}));
        }} 
      />

      {/* Tooltip via Portal */}
      {tooltip.visible && createPortal(
        <div
          className="tm-tooltip"
          style={{ left: tooltip.x + 15, top: tooltip.y + 15 }}
          dangerouslySetInnerHTML={{ __html: tooltip.html }}
        />,
        document.body
      )}

      {/* Deletion Queue Cart Overlay */}
      {cart.length > 0 && (
        <div style={{
          position: 'absolute', right: '20px', bottom: '20px', width: '320px',
          background: 'var(--surface0)', border: '1px solid var(--red)',
          borderRadius: '12px', padding: '16px', zIndex: 50,
          boxShadow: 'var(--shadow-xl)', display: 'flex', flexDirection: 'column', gap: '8px'
        }}>
          <h3 style={{ margin: 0, color: 'var(--red)', display: 'flex', justifyContent: 'space-between', fontSize: '1.1rem' }}>
            Deletion Queue <span style={{ fontSize: '0.8rem', alignSelf: 'center', color: 'var(--text)' }}>{cart.length} item(s)</span>
          </h3>
          <p style={{ margin: 0, fontSize: '0.75rem', color: 'var(--subtext0)' }}>Right-click nodes to add or remove</p>
          <div style={{ maxHeight: '200px', overflowY: 'auto', borderBottom: '1px solid var(--surface1)', paddingBottom: '8px', fontSize: '0.85rem' }}>
            {cart.map(n => (
              <div 
                key={n.path} 
                onClick={() => setCart(c => c.filter(item => item.path !== n.path))}
                title="Click to remove from queue"
                style={{ display: 'flex', justifyContent: 'space-between', padding: '6px 4px', borderBottom: '1px solid rgba(255,255,255,0.05)', cursor: 'pointer', borderRadius: '4px', transition: 'background 0.2s' }}
                onMouseEnter={(e) => e.currentTarget.style.background = 'rgba(243, 139, 168, 0.15)'}
                onMouseLeave={(e) => e.currentTarget.style.background = 'transparent'}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
                  <span style={{ color: 'var(--red)', fontWeight: 'bold', fontSize: '1.1rem', lineHeight: 1 }}>×</span>
                  <span style={{ textOverflow: 'ellipsis', overflow: 'hidden', whiteSpace: 'nowrap', maxWidth: '180px' }}>{n.name}</span>
                </div>
                <span style={{ color: 'var(--subtext0)', paddingLeft: '8px', alignSelf: 'center' }}>{formatBytes(n.size)}</span>
              </div>
            ))}
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: '4px', fontWeight: 'bold', color: 'var(--text)' }}>
            <span>Total:</span>
            <span>{formatBytes(cart.reduce((acc, n) => acc + (n.size || 0), 0))}</span>
          </div>
          <div style={{ display: 'flex', gap: '8px', marginTop: '8px' }}>
            <button 
              onClick={() => setCart([])} 
              disabled={deletingCart}
              style={{ flex: 1, background: 'var(--surface1)', border: '1px solid var(--surface2)', padding: '8px', borderRadius: '8px', color: 'var(--text)', cursor: 'pointer', fontWeight: '600' }}
            >
              Clear
            </button>
            <button 
              onClick={async () => {
                setDeletingCart(true);
                let count = 0;
                for (const item of cart) {
                  try {
                    await invoke("delete_path", { path: item.path });
                    count++;
                  } catch (e) {
                    console.error("Failed to delete", item.path, e);
                  }
                }
                showToast(`Deleted ${count} items. Reloading...`, count < cart.length);
                setCart(c => c.slice(count)); // only keep ones that failed
                setDeletingCart(false);
                loadGraph(focusedPath);
              }}
              disabled={deletingCart}
              style={{ flex: 2, background: 'var(--red)', border: 'none', padding: '8px', borderRadius: '8px', color: 'var(--crust)', cursor: 'pointer', fontWeight: 'bold' }}
            >
              {deletingCart ? 'Emptying...' : 'Delete All'}
            </button>
          </div>
        </div>
      )}

      {toast.visible && createPortal(
        <div className={`delete-toast ${toast.isError ? 'delete-toast-error' : 'delete-toast-success'}`} style={{ zIndex: 100 }}>
          {toast.message}
        </div>,
        document.body
      )}

      {/* Duplicates Modal Overlay */}
      {showDuplicates && (
        <DuplicatesModal 
          rootPath={focusedPath} 
          onClose={() => setShowDuplicates(false)} 
          onAddToCart={async (node) => {
            if (cart.find(n => n.path === node.path)) {
              showToast("Already in deletion queue");
              return;
            }
            try {
              await invoke("check_delete_safety", { path: node.path });
              setCart(c => [...c, node]);
              showToast(`Added ${node.name} to deletion queue`);
            } catch (err) {
              showToast(typeof err === 'string' ? err : "This file is protected", true);
            }
          }}
        />
      )}

    </div>
  );
}
