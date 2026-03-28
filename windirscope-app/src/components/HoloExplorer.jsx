import { useMemo, useState } from 'react';
import { formatBytes } from '../App';
import './HoloExplorer.css';

function HoloNode({ node, level, onNodeClick, onNodeHover, defaultExpanded, selectedNodeId, setSelectedNodeId }) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  
  const hasChildren = node.children && node.children.length > 0;
  const isSelected = selectedNodeId === node.id;

  const handleToggle = (e) => {
    e.stopPropagation();
    
    if (isSelected) {
      // Clicked again while highlighted: toggle expansion
      if (hasChildren) {
        setExpanded(!expanded);
      }
    } else {
      // First click: just select it and fire camera move
      setSelectedNodeId(node.id);
      onNodeClick(node);
    }
  };
  
  const isDir = node.is_dir;
  const icon = isDir ? (expanded ? '📂' : '📁') : '📄';

  return (
    <div className="holo-node-container" style={{ '--indent': level }}>
      <div 
        className={`holo-node-row ${isDir ? 'is-dir' : 'is-file'} ${isSelected ? 'is-selected' : ''}`} 
        onClick={handleToggle}
        onMouseEnter={() => onNodeHover(node)}
        onMouseLeave={() => onNodeHover(null)}
      >
        <span className="holo-node-icon">{icon}</span>
        <span className="holo-node-name" title={node.name}>{node.name}</span>
        <span className="holo-node-size">{formatBytes(node.size)}</span>
      </div>
      
      {hasChildren && expanded && (
        <div className="holo-node-children">
          {node.children.map(child => (
            <HoloNode 
              key={child.id} 
              node={child} 
              level={level + 1} 
              onNodeClick={onNodeClick} 
              onNodeHover={onNodeHover}
              defaultExpanded={false} 
              selectedNodeId={selectedNodeId}
              setSelectedNodeId={setSelectedNodeId}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export default function HoloExplorer({ graphData, onNodeClick, onNodeHover, onGoUp, focusedPath }) {
  const [selectedNodeId, setSelectedNodeId] = useState(null);
  // Compute tree from flat graph
  const treeRoots = useMemo(() => {
    if (!graphData || !graphData.nodes.length) return [];
    
    // Map ID -> Node
    const nodeMap = new Map();
    graphData.nodes.forEach(n => {
      nodeMap.set(n.id, { ...n, children: [] });
    });
    
    // Track roots
    const potentialRoots = new Set(nodeMap.keys());
    
    // Wire up links
    graphData.links.forEach((l) => {
      const srcId = l.source.id || l.source;
      const tgtId = l.target.id || l.target;
      
      const srcNode = nodeMap.get(srcId);
      const tgtNode = nodeMap.get(tgtId);
      
      if (srcNode && tgtNode) {
        srcNode.children.push(tgtNode);
        potentialRoots.delete(tgtId);
      }
    });

    // Sort recursively
    const sortNode = (n) => {
      if (n.children.length > 0) {
        n.children.sort((a,b) => b.size - a.size);
        n.children.forEach(sortNode);
      }
    };
    
    const roots = Array.from(potentialRoots).map(id => nodeMap.get(id));
    roots.sort((a,b) => b.size - a.size);
    roots.forEach(sortNode);
    
    return roots;
  }, [graphData]);

  if (treeRoots.length === 0) return null;

  return (
    <div className="holo-explorer-overlay">
      <div className="holo-explorer-panel">
        <div className="holo-explorer-header">
           <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
             {focusedPath && (
               <button onClick={onGoUp} className="holo-back-btn" title="Go up one level">
                 ◀ Back
               </button>
             )}
             <span>Navigator</span>
           </div>
           <span className="holo-explorer-count">{graphData.nodes.length} nodes</span>
        </div>
        <div className="holo-explorer-tree">
          {treeRoots.map(root => (
            <HoloNode 
              key={root.id} 
              node={root} 
              level={0} 
              onNodeClick={onNodeClick}
              onNodeHover={onNodeHover}
              defaultExpanded={true}
              selectedNodeId={selectedNodeId}
              setSelectedNodeId={setSelectedNodeId}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
