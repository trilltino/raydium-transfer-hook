import { useId } from 'react';
import { formatAmount } from '../lib/amounts.ts';

export interface TokenAmountInputProps {
  label: string;
  tokenLabel: string;
  value: string;
  onChange?: (value: string) => void;
  balance?: bigint | null;
  decimals: number;
  onMax?: () => void;
  onHalf?: () => void;
  readOnly?: boolean;
  invalid?: boolean;
}

export function TokenAmountInput(props: TokenAmountInputProps) {
  const id = useId();
  const { label, tokenLabel, value, onChange, balance, decimals, onMax, onHalf, readOnly, invalid } = props;
  return (
    <div className={`field${invalid ? ' field-invalid' : ''}`}>
      <div className="field-top">
        <label htmlFor={id}>{label}</label>
        {balance !== undefined && balance !== null && (
          <span className="muted" data-testid={`${label.toLowerCase()}-balance`}>
            Balance: {formatAmount(balance, decimals)}
          </span>
        )}
      </div>
      <div className="field-row">
        <input
          id={id}
          className="amount"
          inputMode="decimal"
          autoComplete="off"
          placeholder="0.00"
          value={value}
          readOnly={readOnly}
          aria-invalid={invalid ? true : undefined}
          onChange={(event) => onChange?.(event.target.value)}
        />
        <span className="token-chip" title={tokenLabel}>
          {tokenLabel}
        </span>
      </div>
      {(onMax || onHalf) && (
        <div className="field-actions">
          {onHalf && (
            <button type="button" className="chip" onClick={onHalf}>
              50%
            </button>
          )}
          {onMax && (
            <button type="button" className="chip" onClick={onMax}>
              Max
            </button>
          )}
        </div>
      )}
    </div>
  );
}
