import type { VaultMatrixRow } from "./vaultFleetTypes";

const mark = (value: boolean) => value ? "Yes" : "No";

export default function VaultMatrixPreview({ rows }: { rows: VaultMatrixRow[] }) {
  return (
    <div className="fleet-table-wrap">
      <table className="fleet-policy-table">
        <thead><tr>
          <th>User class</th><th>Volume</th><th>See backing</th><th>Mount</th>
          <th>Decrypt</th><th>Content</th><th>Other session</th>
        </tr></thead>
        <tbody>{rows.map(row => (
          <tr key={`${row.userClass}-${row.volume}`}>
            <td>{row.userClass}</td><td>{row.volume}</td><td>{mark(row.canSeeBacking)}</td>
            <td>{mark(row.canMount)}</td><td>{mark(row.canDecrypt)}</td>
            <td>{mark(row.canAccessContent)}</td><td>{mark(row.seesOtherSessionMount)}</td>
          </tr>
        ))}</tbody>
      </table>
    </div>
  );
}
