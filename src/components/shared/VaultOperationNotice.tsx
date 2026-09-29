import "./VaultOperationNotice.css";

export type VaultNoticeTone = "error" | "success" | "info";

export default function VaultOperationNotice({ message, tone = "error" }: { message: string; tone?: VaultNoticeTone }) {
  if (!message) return null;
  return <div className={`vault-operation-notice vault-operation-notice--${tone}`} role={tone === "error" ? "alert" : "status"}>
    {message}
  </div>;
}
