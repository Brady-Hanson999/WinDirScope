export default function ErrorModal({ errors, skippedPaths, maxErrors, onClose }) {
  const skipped = skippedPaths || [];
  const totalCount = errors.length + skipped.length;

  return (
    <div className="modal-overlay" onClick={(e) => {
      if (e.target === e.currentTarget) onClose();
    }}>
      <div className="modal">
        <div className="modal-header">
          <h2>
            Issues ({totalCount}{errors.length >= maxErrors ? "+" : ""})
          </h2>
          <button className="modal-close-btn" onClick={onClose}>×</button>
        </div>
        <div className="modal-body">
          {skipped.length > 0 && (
            <>
              <div className="modal-section-label">
                Access Denied ({skipped.length})
              </div>
              <ul>
                {skipped.map((p, i) => (
                  <li key={`s-${i}`} className="issue-skipped">{p}</li>
                ))}
              </ul>
            </>
          )}
          {errors.length > 0 && (
            <>
              <div className="modal-section-label" style={{ marginTop: skipped.length > 0 ? 12 : 0 }}>
                Errors ({errors.length}{errors.length >= maxErrors ? "+" : ""})
              </div>
              <ul>
                {errors.map((msg, i) => (
                  <li key={`e-${i}`}>{msg}</li>
                ))}
              </ul>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
