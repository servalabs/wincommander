export default function StartupRecovery({ error, onRetry }: {
  error: string | null;
  onRetry: () => void;
}) {
  if (!error) return null;
  return (
    <div className="sp-startup-error" role="alert">
      <p>{error}</p>
      <button type="button" onClick={onRetry}>Retry startup</button>
    </div>
  );
}
