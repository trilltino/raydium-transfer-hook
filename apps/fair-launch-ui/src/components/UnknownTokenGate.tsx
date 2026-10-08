export interface UnknownTokenGateProps {
  mint: string;
  hookName: string;
  hookProgram: string;
  onConfirm: () => void;
}

/** The mint address is the identity; a symbol could be anything. Shown once per mint until confirmed. */
export function UnknownTokenGate({ mint, hookName, hookProgram, onConfirm }: UnknownTokenGateProps) {
  return (
    <section className="card gate" aria-labelledby="gate-title" data-testid="unknown-token">
      <h2 id="gate-title">Unknown token</h2>
      <dl className="kv">
        <div>
          <dt>Mint</dt>
          <dd className="mono wrap">{mint}</dd>
        </div>
        <div>
          <dt>Hook</dt>
          <dd>{hookName}</dd>
        </div>
        <div>
          <dt>Program</dt>
          <dd className="mono wrap">{hookProgram}</dd>
        </div>
      </dl>
      <p className="muted">
        This token is not in any token list. Check that the mint address is the one you expect before you trade.
      </p>
      <button type="button" className="btn btn-primary" onClick={onConfirm}>
        I understand, continue
      </button>
    </section>
  );
}

const STORAGE_KEY = 'confirmed-mints';

export function readConfirmed(): Set<string> {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    return new Set(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
}

export function rememberConfirmed(mint: string): void {
  try {
    const set = readConfirmed();
    set.add(mint);
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify([...set]));
  } catch {
    // storage can be blocked; the gate then simply shows again next visit
  }
}
