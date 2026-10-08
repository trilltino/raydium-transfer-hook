import { EXPERIMENTAL_NOTICE } from '../config.ts';

export function EnvironmentBadge({ name }: { name: string }) {
  return (
    <span className="badge badge-env" title={EXPERIMENTAL_NOTICE}>
      {name}
    </span>
  );
}

/** Always visible: these are our deployments, not Raydium's. */
export function ExperimentalBanner() {
  return (
    <div className="banner" role="note">
      <strong>Experimental Raydium Transfer Hook environment</strong>
      <span>Not an official Raydium deployment</span>
    </div>
  );
}
