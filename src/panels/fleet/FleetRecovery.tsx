import { useState } from "react";
import { fleetRecovery } from "@/hooks/useFleetRecovery";
import { Button } from "@/components/ui/button";

/** The service verifies the receipt and owns the key; this display is for comparison. */
export default function FleetRecovery() {
  const [receipt, setReceipt] = useState("");
  const [fingerprint, setFingerprint] = useState<string | null>(null);
  const [handoff, setHandoff] = useState<unknown>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [operatorKey, setOperatorKey] = useState("");
  const [operatorFingerprint, setOperatorFingerprint] = useState<string | null>(null);
  const [reviewedOperatorKey, setReviewedOperatorKey] = useState<string | null>(null);
  const [operatorCompared, setOperatorCompared] = useState(false);

  async function reviewOperatorKey() {
    setOperatorFingerprint(null); setReviewedOperatorKey(null); setOperatorCompared(false); setMessage("");
    try {
      const text=operatorKey.trim(); const raw=Uint8Array.from(atob(text), character=>character.charCodeAt(0));
      if (raw.length!==32 || btoa(String.fromCharCode(...raw))!==text) throw new Error();
      const digest=await crypto.subtle.digest("SHA-256",raw);
      setReviewedOperatorKey(text);
      setOperatorFingerprint(Array.from(new Uint8Array(digest), byte=>byte.toString(16).padStart(2,"0")).join(""));
    } catch { setMessage("Enter the complete base64 operator public key from your trusted administrator."); }
  }
  async function pinOperatorKey() {
    if (!operatorFingerprint || !operatorCompared || reviewedOperatorKey!==operatorKey.trim()) return;
    setBusy(true); setMessage("");
    try {
      const result=await fleetRecovery.pinOperatorKey(operatorKey.trim());
      if (!result.pinned) throw new Error();
      setMessage("Operator key saved. Continue recovery; this does not confirm a connection.");
    } catch { setMessage("The operator key was not saved. Check administrator access and the local confirmation."); }
    finally {setBusy(false);}
  }

  async function stage() {
    setMessage(""); setFingerprint(null);
    let retirementReceipt: unknown;
    try {
      if (receipt.length > 8192) throw new Error();
      retirementReceipt = JSON.parse(receipt);
      if (!retirementReceipt || typeof retirementReceipt !== "object" || Array.isArray(retirementReceipt)) throw new Error();
    } catch { setMessage("Paste the complete signed receipt copied from Fleet."); return; }
    setBusy(true);
    try {
      const result = await fleetRecovery.stage(retirementReceipt);
      const key = Uint8Array.from(atob(result.candidate_public_key), (character) => character.charCodeAt(0));
      if (key.length !== 32) throw new Error();
      const digest = await crypto.subtle.digest("SHA-256", key);
      setFingerprint(Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join(""));
      setMessage(result.submission_conflict
        ? "Fleet reported a conflict and may have a different identity waiting. Compare the fingerprint below with the pending identity in Fleet. Do not approve if they differ; reject the incorrect proposal in Fleet, refresh the receipt, and prepare this identity again. A repeated request can also cause a conflict."
        : "The identity proposal was sent. Ask your Fleet administrator to check that it appears and compare this fingerprint before approving it. This does not confirm a connection.");
    } catch { setMessage("The identity proposal could not be sent. Check your connection, the receipt's five-minute expiry, and administrator access, then try again."); }
    finally { setBusy(false); }
  }

  async function resume() {
    setMessage(""); setBusy(true);
    try {
      const result = await fleetRecovery.resume();
      setMessage(result.status === "restart_required"
        ? "Restart Windows, then open WinCommander. Get a fresh retirement receipt from Fleet, check the approved handoff, and select Finish recovery again. The device is not yet reconnected."
        : result.status === "credential_committed_pending_approval"
        ? "Recovery is prepared. Approve this device in Fleet, then check its connection status."
        : "Recovery was resumed. Check Fleet and the device connection status.");
    } catch { setMessage("Recovery could not be resumed. Complete the identity review first, then try again as a local administrator."); }
    finally { setBusy(false); }
  }

  async function fetchHandoff() {
    setMessage(""); setHandoff(null);
    let retirementReceipt: unknown;
    try {
      if (receipt.length > 8192) throw new Error();
      retirementReceipt = JSON.parse(receipt);
      if (!retirementReceipt || typeof retirementReceipt !== "object" || Array.isArray(retirementReceipt)) throw new Error();
    } catch { setMessage("Paste the complete signed receipt copied from Fleet."); return; }
    setBusy(true);
    try {
      const result = await fleetRecovery.handoff(retirementReceipt);
      if (!result || typeof result !== "object" || Array.isArray(result)) throw new Error();
      setHandoff(result);
      setMessage("The approved identity handoff was verified. Select Finish recovery and review the local administrator prompt. The device is not yet reconnected.");
    } catch { setMessage("The handoff could not be verified. Check that Fleet approved this identity and that the receipt has not expired."); }
    finally { setBusy(false); }
  }

  async function finish() {
    if (!handoff) return;
    setMessage(""); setBusy(true);
    try {
      const retirementReceipt: unknown = JSON.parse(receipt);
      const result = await fleetRecovery.finish(retirementReceipt, handoff);
      setMessage(result.status === "restart_required"
        ? "Restart Windows, then open WinCommander. Get a fresh retirement receipt from Fleet, check the approved handoff, and select Finish recovery again. The device is not yet reconnected."
        : result.status === "credential_committed_pending_approval"
        ? "Recovery was prepared. Approve this device in Fleet, then check its connection status."
        : "Recovery was submitted. Check Fleet and the device connection status.");
    } catch { setMessage("Recovery could not be completed. Check the receipt, approval, and local administrator prompt, then try again."); }
    finally { setBusy(false); }
  }

  return <section aria-label="Fleet identity recovery">
    <h4>Recover this device</h4>
    <p>Run WinCommander as a local administrator. To reconnect: (1) verify the operator key, (2) prepare your identity with a fresh retirement receipt, (3) compare the fingerprint with Fleet, and (4) finish the approved handoff.</p>
    <p>Ask your Fleet administrator for a signed retirement receipt. Paste it here within five minutes.</p>
    <p>If recovery is cancelled, the Fleet record remains retired. Restoring the previous identity on this device does not reconnect it to Fleet.</p>
    <p>Get the offline operator public key and its fingerprint from your trusted administrator through an independent channel. Compare the fingerprint before saving it on this device.</p>
    <label className="fleet-field"><span className="fleet-field-label">Offline operator public key</span>
      <textarea className="fleet-input" value={operatorKey} onChange={(event)=>{setOperatorKey(event.target.value);setOperatorFingerprint(null);setReviewedOperatorKey(null);setOperatorCompared(false);}} disabled={busy} />
    </label>
    <Button variant="outline" disabled={busy || !operatorKey.trim()} onClick={()=>void reviewOperatorKey()}>Review operator key</Button>
    {operatorFingerprint && <><p>Operator key fingerprint: <code>{operatorFingerprint}</code></p>
      <label><input type="checkbox" checked={operatorCompared} onChange={event=>setOperatorCompared(event.target.checked)} disabled={busy} /> I compared this fingerprint with my trusted administrator.</label>{" "}
      <Button variant="outline" disabled={busy || !operatorCompared || reviewedOperatorKey!==operatorKey.trim()} onClick={()=>void pinOperatorKey()}>Save operator key</Button></>}
    <label className="fleet-field"><span className="fleet-field-label">Signed retirement receipt</span>
      <textarea className="fleet-input" value={receipt} onChange={(event) => { setReceipt(event.target.value); setHandoff(null); setFingerprint(null); setMessage(""); }} disabled={busy} />
    </label>
    <Button disabled={busy || !receipt.trim()} onClick={() => void stage()}>Prepare identity</Button>{" "}
    <Button variant="outline" disabled={busy || !receipt.trim()} onClick={() => void fetchHandoff()}>Check approved handoff</Button>{" "}
    <Button variant="outline" disabled={busy || !handoff} onClick={() => void finish()}>Finish recovery</Button>{" "}
    <Button variant="outline" disabled={busy} onClick={() => void resume()}>Resume prepared recovery</Button>
    {fingerprint && <p>Replacement fingerprint: <code>{fingerprint}</code></p>}
    {message && <p role="status">{message}</p>}
  </section>;
}
