export interface PolicyMeterProps {
  label: string;
  /** Text for the value reached, already formatted. */
  usedText: string;
  limitText: string;
  /** 0..1, how much of the limit the trade uses. */
  fraction: number;
  violated: boolean;
}

export function PolicyMeter({ label, usedText, limitText, fraction, violated }: PolicyMeterProps) {
  const clamped = Math.min(1, Math.max(0, Number.isFinite(fraction) ? fraction : 0));
  return (
    <div className={`meter${violated ? ' meter-violated' : ''}`} data-testid="policy-meter" data-violated={violated}>
      <div className="meter-head">
        <span>{label}</span>
        <span className={violated ? 'danger' : undefined}>
          {usedText} / {limitText}
        </span>
      </div>
      <div
        className="meter-track"
        role="meter"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(clamped * 100)}
      >
        <div className="meter-fill" style={{ width: `${clamped * 100}%` }} />
      </div>
      {violated && <span className="danger meter-note">Over the limit: the hook will refuse this.</span>}
    </div>
  );
}
