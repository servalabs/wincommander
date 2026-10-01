export default function StartupNotice({ message, onRetry }: {
  message: string | null;
  onRetry: () => void;
}) {
  if (!message) return null;
  return (
    <div className="sp-startup-notice" role="status" aria-live="polite">
      <p>{message}</p>
      <button type="button" onClick={onRetry}>Retry now</button>
    </div>
  );
}
