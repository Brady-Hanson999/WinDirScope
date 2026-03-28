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

export default function ForceGraphView() {
  const fgRef = useRef();
  const [graphData, setGraphData] = useState({ nodes: [], links: [] });
  const [generating, setGenerating] = useState(false);
  const [maxNodes, setMaxNodes] = useState(2000);
  const [focusedPath, setFocusedPath] = useState(null);
  
  const [hoverNode, setHoverNode] = useState(null);
  const [highlightNodes, setHighlightNodes] = useState(new Set());
  const [highlightLinks, setHighlightLinks] = useState(new Set());
  
  const [status, setStatus] = useState("");
  const [tooltip, setTooltip] = useState({ visible: false, x: 0, y: 0, html: "" });

  const loadGraph = useCallback(async (targetPath = null) => {
    setGenerating(true);
    setStatus("Generating 3D Graph...");
    setHighlightNodes(new Set());
    setHighlightLinks(new Set());
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

    if (node.is_dir) {
      const { nodes, links } = getSubtree(node);
      setHighlightNodes(nodes);
      setHighlightLinks(links);
    } else {
      setHighlightNodes(new Set([node.id]));
      setHighlightLinks(new Set());
    }
  }, [getSubtree, loadGraph]);

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
          <button onClick={() => { setHighlightNodes(new Set()); setHighlightLinks(new Set()); }} className="treemap-generate-btn" style={{ background: 'var(--surface1)'}}>
            Clear Highlight
          </button>
        )}

        <span style={{ fontSize: '0.9rem', color: 'var(--subtext0)', alignSelf: 'center' }}>{status}</span>
      </div>

      <ForceGraph3D
        ref={fgRef}
        graphData={graphData}
        numDimensions={2}
        nodeLabel={() => ''} // custom tooltip overlay
        nodeColor={node => {
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

    </div>
  );
}
