import "./App.css";
import ForceGraphView from "./components/ForceGraphView";

export default function ResultsWindow() {
  return (
    <div className="results-window" style={{ width: '100vw', height: '100vh', overflow: 'hidden', margin: 0, padding: 0, position: 'absolute', top: 0, left: 0, right: 0, bottom: 0 }}>
      <ForceGraphView />
    </div>
  );
}
