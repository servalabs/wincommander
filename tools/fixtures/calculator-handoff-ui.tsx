import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import CalculatorGate from "../../src/components/startup/CalculatorGate";

mockWindows("main");
let handoffs = 0;
mockIPC((command) => {
  if (command === "open_windows_calculator") {
    handoffs++;
    const output = document.getElementById("handoffs");
    if (output) output.textContent = String(handoffs);
    if (new URLSearchParams(location.search).has("fail")) throw new Error("Synthetic launch failure");
  }
  if (command === "startup_window_ready") return false;
  if (command === "verify_startup_pin") return "wrong";
  return null;
}, { shouldMockEvents: true });

const output = document.createElement("output");
output.id = "handoffs";
output.textContent = "0";
output.setAttribute("aria-label", "Simulated handoffs");
document.body.append(output);
createRoot(document.getElementById("fixture")!).render(<CalculatorGate generation={1} onAuth={() => {}} />);
