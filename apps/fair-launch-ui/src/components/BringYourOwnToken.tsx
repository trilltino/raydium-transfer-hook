import type { HookEnvironment } from '@raydium-transfer-hook/client';
import type { Connection } from '@solana/web3.js';
import { useState } from 'react';
import { type Approval, type TokenCheck, bringYourHookCommand, checkToken, nextCommands } from '../lib/token-check.ts';
import { AddressLink } from './AddressLink.tsx';

export interface BringYourOwnTokenProps {
  environment: HookEnvironment;
  connection: Connection;
}

const WORDS: Record<Approval, string> = { approved: 'Approved', 'not-approved': 'Not approved', invalid: 'Record is wrong' };

function Command({ title, command }: { title: string; command: string }) {
  return (
    <div className="command">
      <p className="muted small">{title}</p>
      <pre className="mono small" data-testid="command">
        {command}
      </pre>
    </div>
  );
}

/**
 * "Bring your own token": paste a mint and the page reads what is true of it on this network: is it a token, does it
 * have a Transfer Hook, has the pool admin approved it on each AMM (the gate that decides whether a pool may hold it),
 * and the commands that come next, with this environment and token filled in. Nothing here sends a transaction.
 */
export function BringYourOwnToken({ environment, connection }: BringYourOwnTokenProps) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [check, setCheck] = useState<TokenCheck | null>(null);

  const run = async () => {
    setBusy(true);
    try {
      setCheck(await checkToken(connection, environment, text));
    } catch (error) {
      setCheck({ mint: text, kind: 'invalid', message: `Could not read it: ${error instanceof Error ? error.message : String(error)}` });
    } finally {
      setBusy(false);
    }
  };

  const commands = check ? nextCommands(environment, check) : [];
  return (
    <div className="picker-action" data-testid="bring-your-own-token">
      <button type="button" className="btn btn-ghost" aria-expanded={open} onClick={() => setOpen(!open)}>
        Bring your own token
      </button>
      {open && (
        <div className="picker-panel">
          <p className="muted small">
            Paste the mint of a token that has your Transfer Hook. The page checks the hook and whether the pool admin has approved the token on each AMM, which is
            what decides whether a pool may hold it. To build a token and a pool around a hook program of your own, run:
          </p>
          <Command title="Builds your hook, makes a hooked token with it, approves it as the pool admin and creates a pool; --keep-state saves the pool address" command={bringYourHookCommand(environment)} />
          <div className="picker-fields">
            <label className="grow">
              <span className="visually-hidden">Token mint address</span>
              <input className="amount" value={text} placeholder="Token mint address" spellCheck={false} onChange={(event) => setText(event.target.value)} />
            </label>
            <button type="button" className="btn btn-primary" disabled={busy || text.trim() === ''} onClick={() => void run()}>
              {busy ? 'Checking…' : 'Check token'}
            </button>
          </div>
          {check && (
            <div className="token-check" data-testid="token-check" data-kind={check.kind}>
              <p className={check.kind === 'hooked' ? 'rule-passed' : 'muted'}>
                <span className="rule-mark" aria-hidden="true">
                  {check.kind === 'hooked' ? '✓' : '–'}
                </span>
                {check.message}
              </p>
              {check.hookProgram && (
                <p className="muted small">
                  Hook program <AddressLink environment={environment} value={check.hookProgram} /> · hook authority{' '}
                  {check.hookAuthority ? <AddressLink environment={environment} value={check.hookAuthority} /> : 'given up (the hook cannot be changed)'}
                </p>
              )}
              {check.approval && (
                <ul className="address-list" data-testid="token-approval">
                  {(['cpmm', 'clmm'] as const).map((amm) => (
                    <li key={amm} className={check.approval?.[amm] === 'approved' ? 'rule-passed' : 'rule-failed'} data-status={check.approval?.[amm]}>
                      <span className="rule-mark" aria-hidden="true">
                        {check.approval?.[amm] === 'approved' ? '✓' : '✗'}
                      </span>
                      <strong>{amm.toUpperCase()}</strong> <span className="muted">{WORDS[check.approval?.[amm] ?? 'not-approved']}</span>
                    </li>
                  ))}
                </ul>
              )}
              {check.kind === 'plain' && (
                <p className="muted small">A pool of a token without a hook does not need this page: use Raydium. Set a Transfer Hook on the mint first, then check it again.</p>
              )}
              {check.kind === 'hooked' && check.approval?.cpmm === 'approved' && check.approval?.clmm === 'approved' && (
                <p className="muted small">
                  Approved on both AMMs. The tools here make the token and the pool together (the command above); to trade a pool you already have, paste its address in the search bar.
                </p>
              )}
              {commands.map((entry) => (
                <Command key={entry.command} title={entry.title} command={entry.command} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
