import ReactDOM from "react-dom/client";
import { LOGO_URL } from "../assets/logoUrl";

export function mountBackendRequired(): void {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <main className="native-backend-required" role="alert" aria-labelledby="native-backend-required-title">
      <img src={LOGO_URL} alt="" />
      <p className="native-backend-required-kicker">WINCOMMANDER DEVELOPMENT</p>
      <h1 id="native-backend-required-title">Native backend disconnected</h1>
      <p>This browser tab can display the frontend shell, but it cannot read real Windows data or run Tauri commands.</p>
      <code>bun run dev:tauri:free</code>
      <p className="native-backend-required-note">
        Use the WinCommander desktop window opened by that command. Synthetic UI fixtures are isolated at
        <strong> /ui-audit.html</strong>.
      </p>
    </main>,
  );
}
